pub(crate) use scarlet_sys::tls::{
    NATIVE_TLS_CLEANUP_OFFSET as TLS_CLEANUP_OFFSET, NATIVE_TLS_MAPPING_SIZE as TLS_MAPPING_SIZE,
};
use scarlet_sys::tls::{NATIVE_TLS_MAGIC, NATIVE_TLS_SLOT_COUNT, NativeTlsHeader};

use crate::sync::atomic::{AtomicUsize, Ordering};
use crate::sys::pal::abi;
use crate::{mem, ptr};

pub type Key = usize;

// Scarlet does not have native ELF TLS yet, so every Rust `thread_local!`
// consumes one runtime key. rustc alone exceeds the POSIX-minimum 128 keys.
const TLS_KEY_COUNT: usize = NATIVE_TLS_SLOT_COUNT;
const DTOR_ROUNDS: usize = 4;

static NEXT_KEY: AtomicUsize = AtomicUsize::new(1);
static DTORS: [AtomicUsize; TLS_KEY_COUNT] = [const { AtomicUsize::new(0) }; TLS_KEY_COUNT];

// Each statically linked copy of std allocates its own key numbers. The address
// of its destructor table identifies that namespace for the lifetime of the DSO.
// The shared header's first word owns the list. Its errno slot is independent
// of every std namespace. Keep the cleanup record at its established offset;
// loader, executable and DSOs must use this same versioned native TLS layout.
// DSOs must remain loaded until thread exit.
struct Namespace {
    next: *mut Namespace,
    dtors: *const [AtomicUsize; TLS_KEY_COUNT],
    values: [usize; TLS_KEY_COUNT],
}

#[inline]
pub fn create(dtor: Option<unsafe extern "C" fn(*mut u8)>) -> Key {
    let key = NEXT_KEY.fetch_add(1, Ordering::Relaxed);
    if key >= TLS_KEY_COUNT {
        rtabort!("out of TLS keys");
    }
    DTORS[key].store(dtor.map_or(0, |dtor| dtor as usize), Ordering::Release);
    key
}

#[inline]
pub unsafe fn set(key: Key, value: *mut u8) {
    let slot = tls_slot(key);
    unsafe {
        slot.write(value as usize);
    }
}

#[inline]
pub unsafe fn get(key: Key) -> *mut u8 {
    let slot = tls_slot(key);
    unsafe { ptr::with_exposed_provenance_mut(slot.read()) }
}

#[inline]
pub unsafe fn destroy(key: Key) {
    if key < TLS_KEY_COUNT {
        DTORS[key].store(0, Ordering::Release);
    }
}

pub(crate) unsafe fn run_dtors() {
    let head = ptr::with_exposed_provenance_mut::<*mut Namespace>(tls_base());
    for _ in 0..DTOR_ROUNDS {
        let mut any = false;
        let mut namespace = unsafe { *head };
        while !namespace.is_null() {
            for key in 1..TLS_KEY_COUNT {
                // Do not retain references across callbacks: destructors may
                // access TLS, replace values, or prepend another namespace.
                let dtor = unsafe { (*(*namespace).dtors)[key].load(Ordering::Acquire) };
                let value = unsafe { (*namespace).values[key] };
                if dtor != 0 && value != 0 {
                    any = true;
                    unsafe {
                        (*namespace).values[key] = 0;
                        mem::transmute::<usize, unsafe extern "C" fn(*mut u8)>(dtor)(
                            ptr::with_exposed_provenance_mut(value),
                        );
                    }
                }
            }
            namespace = unsafe { (*namespace).next };
        }

        if !any {
            break;
        }
    }
    // Values that keep reinstalling themselves after DTOR_ROUNDS have the same
    // bounded-cleanup behavior as before. Reclaim the per-thread tables; the
    // thread mapping itself is released by native thread exit / process exit.
    let mut namespace = unsafe { head.replace(ptr::null_mut()) };
    while !namespace.is_null() {
        let next = unsafe { (*namespace).next };
        unsafe { drop(Box::from_raw(namespace)) };
        namespace = next;
    }
}

#[inline]
fn tls_slot(key: Key) -> *mut usize {
    if key == 0 || key >= TLS_KEY_COUNT {
        rtabort!("invalid TLS key");
    }
    let head = ptr::with_exposed_provenance_mut::<*mut Namespace>(tls_base());
    // The head and list are accessed exclusively by their owning thread. Heap
    // allocation here uses Scarlet's native allocator, which does not use TLS.
    unsafe {
        let mut namespace = *head;
        while !namespace.is_null() {
            if (*namespace).dtors == &raw const DTORS {
                return &raw mut (*namespace).values[key];
            }
            namespace = (*namespace).next;
        }
        let namespace = Box::into_raw(Box::new(Namespace {
            next: *head,
            dtors: &raw const DTORS,
            values: [0; TLS_KEY_COUNT],
        }));
        *head = namespace;
        &raw mut (*namespace).values[key]
    }
}

fn tls_base() -> usize {
    // Keep the Rust TLS fallback for code entered without the Scarlet CRT.
    ensure_native_tls()
}

/// Initialize the initial thread before any environment or constructor code.
/// A loader and executable may both enter here on the same thread; preserve
/// the established header, errno and namespace list in that case.
pub(crate) fn ensure_native_tls() -> usize {
    let base = arch_tls_pointer();
    if base == 0 {
        main_tls_base()
    } else {
        checked_native_tls(base);
        base
    }
}

/// Return the shared C errno slot without allocating or lazily creating TLS.
#[inline]
pub(crate) fn native_errno_location() -> *mut i32 {
    let header = checked_native_tls(arch_tls_pointer());
    // SAFETY: validation established the current thread's initialized header.
    unsafe { &raw mut (*header).errno }
}

#[inline]
fn checked_native_tls(base: usize) -> *mut NativeTlsHeader {
    if base == 0 {
        core::intrinsics::abort();
    }
    let header = ptr::with_exposed_provenance_mut::<NativeTlsHeader>(base);
    // SAFETY: a nonzero native thread pointer addresses its runtime TLS mapping.
    // Reject older inline-key or otherwise incompatible layouts before using
    // any namespace pointer. This failure path must not allocate or access TLS.
    if unsafe { (*header).magic } != NATIVE_TLS_MAGIC {
        core::intrinsics::abort();
    }
    header
}

fn main_tls_base() -> usize {
    let new_base = abi::memory_map(
        0,
        0,
        TLS_MAPPING_SIZE,
        abi::mmap::PROT_READ | abi::mmap::PROT_WRITE,
        abi::mmap::MAP_PRIVATE | abi::mmap::MAP_ANONYMOUS,
        0,
    )
    .unwrap_or_else(|()| core::intrinsics::abort());
    if new_base == 0 {
        core::intrinsics::abort();
    }
    // SAFETY: this is a new writable mapping exclusively owned by this thread.
    unsafe {
        ptr::with_exposed_provenance_mut::<NativeTlsHeader>(new_base)
            .write(NativeTlsHeader::INITIAL);
    }

    // SetTls publishes the initialized mapping in both the kernel's ABI state
    // and the architectural thread pointer. A register-only write would leave
    // the kernel with stale state during scheduling or subsequent native calls.
    if scarlet_sys::syscall1(scarlet_sys::Syscall::SetTls, new_base) != 0 {
        let _ = abi::memory_unmap(new_base, TLS_MAPPING_SIZE);
        core::intrinsics::abort();
    }
    new_base
}

#[cfg(target_arch = "aarch64")]
#[inline]
fn arch_tls_pointer() -> usize {
    let tpidr_el0: usize;
    unsafe {
        core::arch::asm!(
            "mrs {}, tpidr_el0",
            out(reg) tpidr_el0,
            options(nostack, readonly)
        );
    }
    tpidr_el0
}

#[cfg(any(target_arch = "riscv32", target_arch = "riscv64"))]
#[inline]
fn arch_tls_pointer() -> usize {
    let tp;
    unsafe {
        core::arch::asm!(
            "mv {}, tp",
            out(reg) tp,
            options(nostack, readonly)
        );
    }
    tp
}
