//! Scarlet Native command line argument storage.

pub use super::common::Args;
use crate::ffi::{CStr, OsString};
use crate::ptr;
use crate::sync::atomic::{Atomic, AtomicIsize, AtomicPtr, Ordering};
use crate::sys::{FromInner, os_str};

static ARGC: Atomic<isize> = AtomicIsize::new(0);
static ARGV: Atomic<*mut *const u8> = AtomicPtr::new(ptr::null_mut());

/// One-time global initialization.
///
/// # Safety
///
/// `argv` must either be null or point to at least `argc` valid C string
/// pointers for the lifetime of the process.
pub unsafe fn init(argc: isize, argv: *const *const u8) {
    ARGC.store(argc, Ordering::Relaxed);
    ARGV.store(argv.cast_mut(), Ordering::Relaxed);
}

/// Returns the command line arguments.
pub fn args() -> Args {
    let argv = ARGV.load(Ordering::Relaxed);
    let argc = if argv.is_null() { 0 } else { ARGC.load(Ordering::Relaxed).max(0) };
    let mut args = Vec::with_capacity(argc as usize);

    for i in 0..argc {
        // SAFETY: `init` stores the kernel-provided argv pointer. A non-null
        // argv is valid for at least `argc` elements.
        let ptr = unsafe { argv.offset(i).read() };
        if ptr.is_null() {
            break;
        }

        // SAFETY: Scarlet argv entries are null-terminated byte strings.
        let bytes = unsafe { CStr::from_ptr(ptr.cast()) }.to_bytes().to_vec();
        args.push(OsString::from_inner(os_str::Buf { inner: bytes }));
    }

    Args::new(args)
}
