use crate::sync::atomic::{Atomic, Ordering};
use crate::sys::pal::abi;
use crate::time::Duration;

/// An atomic for use as a futex that is at least 32 bits but may be larger.
pub type Futex = Atomic<Primitive>;
/// Must be the underlying type of Futex.
pub type Primitive = u32;

/// An atomic for use as a futex that is at least 8 bits but may be larger.
pub type SmallFutex = Atomic<SmallPrimitive>;
/// Must be the underlying type of SmallFutex.
pub type SmallPrimitive = u32;

pub fn futex_wait(futex: &Atomic<u32>, expected: u32, timeout: Option<Duration>) -> bool {
    if futex.load(Ordering::Relaxed) != expected {
        return true;
    }

    if let Some(timeout) = timeout {
        if timeout.is_zero() {
            return false;
        }

        // TODO(scarlet): replace this polling fallback with a kernel futex or
        // wait-address syscall. This is only meant to make std synchronization
        // primitives correct enough to use while the kernel primitive is absent.
        let nanos = timeout.as_nanos().min(1_000_000) as u64;
        let _ = abi::sleep(nanos);
        return futex.load(Ordering::Relaxed) != expected;
    }

    while futex.load(Ordering::Relaxed) == expected {
        let _ = abi::thread_yield();
        let _ = abi::sleep(1_000_000);
    }
    true
}

pub fn futex_wake(_futex: &Atomic<u32>) -> bool {
    // TODO(scarlet): wake one waiter once the kernel exposes futex support.
    false
}

pub fn futex_wake_all(_futex: &Atomic<u32>) {
    // TODO(scarlet): wake all waiters once the kernel exposes futex support.
}
