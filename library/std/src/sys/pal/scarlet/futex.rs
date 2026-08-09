use crate::sync::atomic::{Atomic, Ordering};
use crate::sys::pal::abi;
use crate::time::Duration;

const FUTEX_TIMED_OUT: usize = 2;
const FUTEX_WAIT_FOREVER: usize = usize::MAX;
const FALLBACK_POLL_NS: u64 = 10_000_000;

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

    if timeout.is_some_and(|duration| duration.is_zero()) {
        return false;
    }

    let timeout_ns = timeout.map_or(FUTEX_WAIT_FOREVER, |duration| {
        duration.as_nanos().min((usize::MAX - 1) as u128) as usize
    });
    match abi::futex_wait(futex.as_ptr(), expected, timeout_ns) {
        Ok(FUTEX_TIMED_OUT) => false,
        Ok(_) => true,
        Err(()) => {
            // A newer standard library may temporarily run on a kernel that
            // predates FutexWait. Preserve correctness without turning that
            // version skew into a tight syscall loop.
            let fallback_ns = timeout
                .map(|duration| duration.as_nanos().min(u64::MAX as u128) as u64)
                .unwrap_or(FALLBACK_POLL_NS);
            let _ = abi::sleep(fallback_ns);
            timeout.is_none() || futex.load(Ordering::Relaxed) != expected
        }
    }
}

pub fn futex_wake(futex: &Atomic<u32>) -> bool {
    abi::futex_wake(futex.as_ptr(), 1).is_ok_and(|woken| woken != 0)
}

pub fn futex_wake_all(futex: &Atomic<u32>) {
    let _ = abi::futex_wake(futex.as_ptr(), usize::MAX);
}
