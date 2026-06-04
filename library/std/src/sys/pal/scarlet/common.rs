use crate::ffi::c_char;
use crate::io as std_io;

// SAFETY: must be called only once during runtime initialization.
// NOTE: this is not guaranteed to run, for example when Rust code is called externally.
pub unsafe fn init(argc: isize, argv: *const *const u8, _sigpipe: u8) {
    // SAFETY: `rt::lang_start` forwards the process argc/argv pair that was
    // passed to the compiler-generated `main` shim.
    unsafe {
        crate::sys::args::init(argc, argv);
    }
}

// SAFETY: must be called only once during runtime cleanup.
// NOTE: this is not guaranteed to run, for example when the program aborts.
pub unsafe fn cleanup() {}

pub fn unsupported<T>() -> std_io::Result<T> {
    Err(unsupported_err())
}

pub fn unsupported_err() -> std_io::Error {
    std_io::Error::UNSUPPORTED_PLATFORM
}

pub fn is_interrupted(_code: i32) -> bool {
    false
}

pub fn decode_error_kind(_code: i32) -> crate::io::ErrorKind {
    crate::io::ErrorKind::Uncategorized
}

pub fn abort_internal() -> ! {
    core::intrinsics::abort();
}

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn _start(argc: isize, argv: *const *const c_char) -> ! {
    unsafe extern "C" {
        fn main(argc: i32, argv: *const *const c_char) -> i32;
    }

    let envp = envp_from_argv(argc, argv);
    crate::sys::env::init(envp);

    // SAFETY: rustc emits `main` as the C ABI entry shim for normal Rust
    // executables. It calls `std::rt::lang_start`, which runs `sys::init`.
    let code = unsafe { main(argc as i32, argv) };
    #[cfg(all(
        target_thread_local,
        not(all(target_family = "wasm", not(target_feature = "atomics")))
    ))]
    crate::sys::thread_local::destructors::run();
    crate::rt::thread_cleanup();
    crate::sys::pal::os::exit(code);
}

fn envp_from_argv(argc: isize, argv: *const *const c_char) -> *const *const c_char {
    if argc < 0 || argv.is_null() {
        return crate::ptr::null();
    }

    // SAFETY: The Scarlet process ABI passes a null-terminated argv array,
    // immediately followed by a null-terminated envp array.
    unsafe { argv.add(argc as usize + 1) }
}
