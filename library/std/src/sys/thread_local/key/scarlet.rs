use crate::sync::atomic::{AtomicUsize, Ordering};
use crate::sys::pal::abi;
use crate::{mem, ptr};

pub type Key = usize;

const PAGE_SIZE: usize = 4096;
const TLS_KEY_COUNT: usize = 128;
const POINTER_SIZE: usize = mem::size_of::<usize>();
const DTOR_ROUNDS: usize = 4;

static NEXT_KEY: AtomicUsize = AtomicUsize::new(1);
static MAIN_TLS_BASE: AtomicUsize = AtomicUsize::new(0);
static DTORS: [AtomicUsize; TLS_KEY_COUNT] = [const { AtomicUsize::new(0) }; TLS_KEY_COUNT];

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
    for _ in 0..DTOR_ROUNDS {
        let mut any = false;
        for key in 1..TLS_KEY_COUNT {
            let dtor = DTORS[key].load(Ordering::Acquire);
            if dtor == 0 {
                continue;
            }

            let value = unsafe { get(key) };
            if value.is_null() {
                continue;
            }

            any = true;
            unsafe {
                set(key, ptr::null_mut());
                mem::transmute::<usize, unsafe extern "C" fn(*mut u8)>(dtor)(value);
            }
        }

        if !any {
            break;
        }
    }
}

#[inline]
fn tls_slot(key: Key) -> *mut usize {
    if key >= TLS_KEY_COUNT {
        rtabort!("invalid TLS key");
    }
    ptr::with_exposed_provenance_mut::<usize>(tls_base() + key * POINTER_SIZE)
}

fn tls_base() -> usize {
    let base = arch_tls_pointer();
    if base != 0 { base } else { main_tls_base() }
}

fn main_tls_base() -> usize {
    let base = MAIN_TLS_BASE.load(Ordering::Acquire);
    if base != 0 {
        return base;
    }

    let new_base = abi::memory_map(
        0,
        0,
        PAGE_SIZE,
        abi::mmap::PROT_READ | abi::mmap::PROT_WRITE,
        abi::mmap::MAP_PRIVATE | abi::mmap::MAP_ANONYMOUS,
        0,
    )
    .unwrap_or_else(|()| rtabort!("failed to allocate main thread TLS"));

    match MAIN_TLS_BASE.compare_exchange(0, new_base, Ordering::Release, Ordering::Acquire) {
        Ok(_) => new_base,
        Err(existing) => {
            let _ = abi::memory_unmap(new_base, PAGE_SIZE);
            existing
        }
    }
}

#[cfg(target_arch = "aarch64")]
#[inline]
fn arch_tls_pointer() -> usize {
    let tpidr_el0: usize;
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
    unsafe {
        core::arch::asm!(
            "mv {}, tp",
            out(reg) tp,
            options(nostack, pure, readonly)
        );
    }
    tp
}
