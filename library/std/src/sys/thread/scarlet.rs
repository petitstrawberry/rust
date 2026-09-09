use crate::ffi::CStr;
use crate::mem::ManuallyDrop;
use crate::num::NonZero;
use crate::sys::pal::abi;
use crate::thread::ThreadInit;
use crate::time::Duration;
use crate::{cmp, io, ptr};

const PAGE_SIZE: usize = 4096;
const STACK_ALIGN: usize = 16;
const TLS_MAPPING_SIZE: usize = PAGE_SIZE;
const TLS_CLEANUP_OFFSET: usize = 2048;
const THREAD_CLEANUP_MAGIC: u64 = 0x5343_5448_5244_0001;

pub const DEFAULT_MIN_STACK_SIZE: usize = 64 * 1024;

#[repr(C)]
#[derive(Clone, Copy)]
struct ThreadCleanupRecord {
    magic: u64,
    stack_mapping_base: usize,
    stack_mapping_len: usize,
    tls_mapping_base: usize,
    tls_mapping_len: usize,
}

struct ThreadStart {
    init: Box<ThreadInit>,
    stack_mapping_base: usize,
    stack_mapping_len: usize,
    tls_mapping_base: usize,
    tls_mapping_len: usize,
}

struct ThreadStackMapping {
    mapping_base: usize,
    mapping_len: usize,
    stack_base: usize,
    stack_len: usize,
}

pub struct Thread {
    tid: u32,
}

unsafe impl Send for Thread {}
unsafe impl Sync for Thread {}

impl Thread {
    // unsafe: see thread::Builder::spawn_unchecked for safety requirements
    pub unsafe fn new(stack: usize, init: Box<ThreadInit>) -> io::Result<Thread> {
        let stack = allocate_thread_stack(cmp::max(stack, DEFAULT_MIN_STACK_SIZE))?;
        let stack_top = (stack.stack_base + stack.stack_len - 16) & !(STACK_ALIGN - 1);
        let tls_ptr = match allocate_thread_tls() {
            Ok(ptr) => ptr,
            Err(err) => {
                let _ = abi::memory_unmap(stack.mapping_base, stack.mapping_len);
                return Err(err);
            }
        };
        write_thread_cleanup_record(tls_ptr, stack.mapping_base, stack.mapping_len);

        let start = Box::new(ThreadStart {
            init,
            stack_mapping_base: stack.mapping_base,
            stack_mapping_len: stack.mapping_len,
            tls_mapping_base: tls_ptr,
            tls_mapping_len: TLS_MAPPING_SIZE,
        });
        let start_ptr = Box::into_raw(start).expose_provenance();

        let flags = abi::clone_flags::THREAD
            | abi::clone_flags::SET_TLS
            | abi::clone_flags::VM
            | abi::clone_flags::FS
            | abi::clone_flags::FILES;

        match abi::clone_thread(flags, stack_top, thread_start, start_ptr, tls_ptr) {
            Ok(tid) => Ok(Thread { tid }),
            Err(()) => {
                // SAFETY: the kernel did not consume `start_ptr` if clone failed.
                let start = unsafe {
                    Box::from_raw(ptr::with_exposed_provenance_mut::<ThreadStart>(start_ptr))
                };
                cleanup_thread_mappings(
                    start.stack_mapping_base,
                    start.stack_mapping_len,
                    start.tls_mapping_base,
                    start.tls_mapping_len,
                );
                Err(io::ErrorKind::Other.into())
            }
        }
    }

    pub fn join(self) {
        let tid = ManuallyDrop::new(self).tid;
        let mut status = 0;
        let pid = abi::waitpid(tid as i32, &mut status, 0).expect("failed to join Scarlet thread");
        assert_eq!(pid, tid as i32, "joined unexpected Scarlet thread");
    }
}

impl Drop for Thread {
    fn drop(&mut self) {
        let _ = abi::thread_detach(self.tid);
    }
}

pub fn available_parallelism() -> io::Result<NonZero<usize>> {
    // TODO(scarlet): expose the online CPU count from the kernel.
    Ok(NonZero::<usize>::MIN)
}

pub fn current_os_id() -> Option<u64> {
    // TODO(scarlet): expose a Native thread id distinct from the process id.
    None
}

pub fn yield_now() {
    let _ = abi::thread_yield();
}

#[allow(dead_code)]
pub fn set_name(_name: &CStr) {
    // TODO(scarlet): expose a Native thread naming operation.
}

pub fn sleep(dur: Duration) {
    let nanos = dur.as_nanos().min(u64::MAX as u128) as u64;
    let _ = abi::sleep(nanos);
}

fn allocate_thread_stack(stack_size: usize) -> io::Result<ThreadStackMapping> {
    let mapping_len = stack_size + PAGE_SIZE;
    let mapping_base = abi::memory_map(
        0,
        0,
        mapping_len,
        abi::mmap::PROT_NONE,
        abi::mmap::MAP_PRIVATE | abi::mmap::MAP_ANONYMOUS,
        0,
    )
    .map_err(|()| io::ErrorKind::Other)?;
    let stack_base = mapping_base + PAGE_SIZE;

    if abi::memory_map(
        0,
        stack_base,
        stack_size,
        abi::mmap::PROT_READ | abi::mmap::PROT_WRITE,
        abi::mmap::MAP_PRIVATE | abi::mmap::MAP_ANONYMOUS | abi::mmap::MAP_FIXED,
        0,
    )
    .is_err()
    {
        let _ = abi::memory_unmap(mapping_base, mapping_len);
        return Err(io::ErrorKind::Other.into());
    }

    Ok(ThreadStackMapping { mapping_base, mapping_len, stack_base, stack_len: stack_size })
}

fn allocate_thread_tls() -> io::Result<usize> {
    abi::memory_map(
        0,
        0,
        TLS_MAPPING_SIZE,
        abi::mmap::PROT_READ | abi::mmap::PROT_WRITE,
        abi::mmap::MAP_PRIVATE | abi::mmap::MAP_ANONYMOUS,
        0,
    )
    .map_err(|()| io::ErrorKind::Other.into())
}

fn cleanup_thread_mappings(
    stack_mapping_base: usize,
    stack_mapping_len: usize,
    tls_mapping_base: usize,
    tls_mapping_len: usize,
) {
    let _ = abi::memory_unmap(stack_mapping_base, stack_mapping_len);
    let _ = abi::memory_unmap(tls_mapping_base, tls_mapping_len);
}

fn write_thread_cleanup_record(
    tls_mapping_base: usize,
    stack_mapping_base: usize,
    stack_mapping_len: usize,
) {
    let record = ThreadCleanupRecord {
        magic: THREAD_CLEANUP_MAGIC,
        stack_mapping_base,
        stack_mapping_len,
        tls_mapping_base,
        tls_mapping_len: TLS_MAPPING_SIZE,
    };
    // SAFETY: `tls_mapping_base` points to a writable page we just mapped.
    unsafe {
        ptr::write(
            ptr::with_exposed_provenance_mut::<ThreadCleanupRecord>(
                tls_mapping_base + TLS_CLEANUP_OFFSET,
            ),
            record,
        );
    }
}

extern "C" fn thread_start(start_ptr: usize) -> ! {
    // SAFETY: parent allocated this packet and transfers ownership to the child
    // by passing it as the clone entry argument.
    let start =
        unsafe { Box::from_raw(ptr::with_exposed_provenance_mut::<ThreadStart>(start_ptr)) };
    let ThreadStart { init, .. } = *start;
    let rust_start = init.init();
    rust_start();

    #[cfg(target_os = "scarlet")]
    unsafe {
        crate::sys::thread_local::key::run_dtors();
    }
    #[cfg(all(
        target_thread_local,
        not(all(target_family = "wasm", not(target_feature = "atomics")))
    ))]
    unsafe {
        crate::sys::thread_local::destructors::run();
    }
    #[cfg(not(target_os = "scarlet"))]
    crate::rt::thread_cleanup();
    exit_current_thread(0);
}

fn exit_current_thread(code: i32) -> ! {
    let tls_base = arch_tls_pointer();
    if tls_base != 0 {
        // SAFETY: when present, Scarlet thread TLS contains this runtime-owned
        // cleanup record at `TLS_CLEANUP_OFFSET`.
        let record = unsafe {
            ptr::read(ptr::with_exposed_provenance::<ThreadCleanupRecord>(
                tls_base + TLS_CLEANUP_OFFSET,
            ))
        };
        if record.magic == THREAD_CLEANUP_MAGIC {
            abi::thread_exit_cleanup(
                code,
                record.stack_mapping_base,
                record.stack_mapping_len,
                record.tls_mapping_base,
                record.tls_mapping_len,
            );
        }
    }

    abi::exit_current_thread(code);
}

#[cfg(target_arch = "aarch64")]
#[inline]
fn arch_tls_pointer() -> usize {
    let tpidr_el0: usize;
    // SAFETY: reading the userspace TLS register has no side effects.
    unsafe {
        core::arch::asm!(
            "mrs {}, tpidr_el0",
            out(reg) tpidr_el0,
            options(nostack, pure, readonly)
        );
    }
    tpidr_el0
}

#[cfg(any(target_arch = "riscv32", target_arch = "riscv64"))]
#[inline]
fn arch_tls_pointer() -> usize {
    let tp;
    // SAFETY: reading the userspace TLS register has no side effects.
    unsafe {
        core::arch::asm!(
            "mv {}, tp",
            out(reg) tp,
            options(nostack, pure, readonly)
        );
    }
    tp
}
