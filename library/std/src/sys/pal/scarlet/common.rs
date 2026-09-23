use core::sync::atomic::{AtomicPtr, Ordering};

use crate::ffi::c_char;
use crate::io as std_io;

// The kernel keeps the initial stack mapped for the life of the process.
// Publish auxv before calling constructors, which may detect CPU features.
// A custom entry point that skips `__scarlet_start` keeps the outline helper's safe
// LL/SC default unless it performs equivalent auxv and constructor setup.
static AUXV: AtomicPtr<usize> = AtomicPtr::new(core::ptr::null_mut());

#[unsafe(no_mangle)]
pub extern "C" fn __scarlet_getauxval(key: usize) -> usize {
    let mut entry = AUXV.load(Ordering::Acquire);
    if entry.is_null() {
        return 0;
    }
    // SAFETY: The kernel places a terminated array of native-word pairs after
    // envp on the persistent initial process stack.
    unsafe {
        while *entry != 0 {
            if *entry == key {
                return *entry.add(1);
            }
            entry = entry.add(2);
        }
    }
    0
}

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

pub fn is_interrupted(code: i32) -> bool {
    code == scarlet_sys::ERRNO_EINTR
}

pub fn decode_error_kind(code: i32) -> crate::io::ErrorKind {
    use scarlet_sys::fs::*;
    match code {
        ERRNO_ENOENT => crate::io::ErrorKind::NotFound,
        ERRNO_EACCES => crate::io::ErrorKind::PermissionDenied,
        ERRNO_EEXIST => crate::io::ErrorKind::AlreadyExists,
        ERRNO_ENOTDIR => crate::io::ErrorKind::NotADirectory,
        ERRNO_EISDIR => crate::io::ErrorKind::IsADirectory,
        ERRNO_ENOSPC => crate::io::ErrorKind::StorageFull,
        ERRNO_EROFS => crate::io::ErrorKind::ReadOnlyFilesystem,
        ERRNO_ENAMETOOLONG => crate::io::ErrorKind::InvalidFilename,
        ERRNO_ENOTEMPTY => crate::io::ErrorKind::DirectoryNotEmpty,
        ERRNO_ELOOP => crate::io::ErrorKind::FilesystemLoop,
        ERRNO_EOVERFLOW => crate::io::ErrorKind::FileTooLarge,
        scarlet_sys::ERRNO_EINTR => crate::io::ErrorKind::Interrupted,
        scarlet_sys::ERRNO_EIO => crate::io::ErrorKind::Other,
        scarlet_sys::ERRNO_EAGAIN => crate::io::ErrorKind::WouldBlock,
        scarlet_sys::ERRNO_EINVAL => crate::io::ErrorKind::InvalidInput,
        scarlet_sys::ERRNO_EMSGSIZE => crate::io::ErrorKind::InvalidInput,
        scarlet_sys::ERRNO_EPROTONOSUPPORT | scarlet_sys::ERRNO_EOPNOTSUPP => {
            crate::io::ErrorKind::Unsupported
        }
        scarlet_sys::ERRNO_EADDRINUSE => crate::io::ErrorKind::AddrInUse,
        scarlet_sys::ERRNO_EADDRNOTAVAIL => crate::io::ErrorKind::AddrNotAvailable,
        scarlet_sys::ERRNO_ENETUNREACH => crate::io::ErrorKind::NetworkUnreachable,
        scarlet_sys::ERRNO_ECONNABORTED => crate::io::ErrorKind::ConnectionAborted,
        scarlet_sys::ERRNO_ECONNRESET => crate::io::ErrorKind::ConnectionReset,
        scarlet_sys::ERRNO_ENOTCONN => crate::io::ErrorKind::NotConnected,
        scarlet_sys::ERRNO_ETIMEDOUT => crate::io::ErrorKind::TimedOut,
        scarlet_sys::ERRNO_ECONNREFUSED => crate::io::ErrorKind::ConnectionRefused,
        _ => crate::io::ErrorKind::Uncategorized,
    }
}

pub fn abort_internal() -> ! {
    core::intrinsics::abort();
}

// Only the executable's CRT references `main` and linker-defined constructor
// boundaries. Keeping those references out of std lets a Rust dylib embed std
// without acquiring an entry point or unresolved executable-only symbols.
//
// SAFETY: The executable CRT passes the persistent kernel argc/argv/envp/auxv
// arrays, its C ABI main shim and the linker-defined constructor range. This
// function is entered once, before any application code runs.
#[cfg(not(test))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __scarlet_start(
    argc: isize,
    argv: *const *const c_char,
    envp: *const *const c_char,
    auxv: *const usize,
    main: unsafe extern "C" fn(i32, *const *const c_char) -> i32,
    init_array_start: *const unsafe extern "C" fn(),
    init_array_end: *const unsafe extern "C" fn(),
) -> ! {
    crate::sys::thread_local::key::ensure_native_tls();
    AUXV.store(auxv.cast_mut(), Ordering::Release);
    crate::sys::env::init(envp);

    // The loader initializes dependency DSOs. Main-image constructors remain
    // the executable CRT's responsibility on both Scarlet architectures.
    let mut entry = init_array_start;
    while entry < init_array_end {
        // SAFETY: The CRT supplies the executable's array of C constructors.
        unsafe { (*entry)() };
        // SAFETY: The next pointer stays within or one past the constructor array.
        entry = unsafe { entry.add(1) };
    }

    // SAFETY: rustc's executable main shim calls std::rt::lang_start, which
    // initializes process arguments through sys::init before calling user main.
    let code = unsafe { main(argc as i32, argv) };
    #[cfg(target_os = "scarlet")]
    unsafe {
        crate::sys::thread_local::key::run_dtors();
    }
    #[cfg(all(
        target_thread_local,
        not(all(target_family = "wasm", not(target_feature = "atomics")))
    ))]
    crate::sys::thread_local::destructors::run();
    #[cfg(not(target_os = "scarlet"))]
    crate::rt::thread_cleanup();
    crate::sys::pal::os::exit(code);
}

// RV32 remains static-only and retains its existing Rust entry. The separate
// executable CRT is enabled only for the native 64-bit targets.
#[cfg(all(not(test), target_pointer_width = "32"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn _start(argc: isize, argv: *const *const c_char) -> ! {
    unsafe extern "C" {
        fn main(argc: i32, argv: *const *const c_char) -> i32;
    }
    let (envp, auxv) = if argc < 0 || argv.is_null() {
        (crate::ptr::null(), crate::ptr::null())
    } else {
        // SAFETY: Scarlet places envp after argv, then auxv after envp's NULL.
        unsafe {
            let envp = argv.add(argc as usize + 1);
            let mut end = envp;
            while !(*end).is_null() {
                end = end.add(1);
            }
            (envp, end.add(1).cast())
        }
    };
    // SAFETY: Forward the kernel arrays and rustc's main shim. RV32 previously
    // did not run an init array, so its empty constructor range stays unchanged.
    unsafe { __scarlet_start(argc, argv, envp, auxv, main, crate::ptr::null(), crate::ptr::null()) }
}
