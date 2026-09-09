//! Unsafe Scarlet Native syscall bindings.
//!
//! This crate is the raw syscall layer for Scarlet userland. It is deliberately
//! small: syscall numbers and ABI types live in `scarlet-abi`, while safe
//! object wrappers live above this crate.

#![no_std]

pub use scarlet_abi::{
    ERRNO_EADDRINUSE, ERRNO_EADDRNOTAVAIL, ERRNO_EAGAIN, ERRNO_EBADF, ERRNO_ECONNABORTED,
    ERRNO_ECONNREFUSED, ERRNO_ECONNRESET, ERRNO_EINTR, ERRNO_EINVAL, ERRNO_EIO, ERRNO_EISCONN,
    ERRNO_EMSGSIZE, ERRNO_ENETUNREACH, ERRNO_ENOTCONN, ERRNO_EOPNOTSUPP, ERRNO_EPROTONOSUPPORT,
    ERRNO_ETIMEDOUT, FILE_PERMISSION_EXECUTE, FILE_PERMISSION_READ, FILE_PERMISSION_WRITE,
    FILE_TYPE_BLOCK_DEVICE, FILE_TYPE_CHAR_DEVICE, FILE_TYPE_DIRECTORY, FILE_TYPE_PIPE,
    FILE_TYPE_REGULAR, FILE_TYPE_SOCKET, FILE_TYPE_SYMLINK, FILE_TYPE_UNKNOWN, Pid,
    RawFileMetadata, RawHandle, SCTL_SOCKET_GET_NONBLOCK, SCTL_SOCKET_GET_READ_TIMEOUT_MS,
    SCTL_SOCKET_GET_WRITE_TIMEOUT_MS, SCTL_SOCKET_SET_NONBLOCK, SCTL_SOCKET_SET_READ_TIMEOUT_MS,
    SCTL_SOCKET_SET_WRITE_TIMEOUT_MS, SCTL_SOCKET_TAKE_ERROR, Syscall, Tid,
};

#[cfg(target_arch = "aarch64")]
#[path = "arch/aarch64.rs"]
mod arch;
#[cfg(any(target_arch = "riscv32", target_arch = "riscv64"))]
#[path = "arch/riscv.rs"]
mod arch;

pub use scarlet_abi::native_scalar;

/// Invoke a wide-result syscall with up to six Native argument words.
/// RV32 returns the low/high halves in a0/a1; 64-bit targets return one word.
pub fn syscall_u64(syscall: Syscall, args: [usize; 6]) -> u64 {
    #[cfg(target_pointer_width = "64")]
    {
        arch::syscall6(syscall, args[0], args[1], args[2], args[3], args[4], args[5]) as u64
    }
    #[cfg(target_arch = "riscv32")]
    {
        arch::syscall_u64(syscall, args)
    }
}

/// Invoke a Scarlet Native syscall with no arguments.
pub fn syscall0(syscall: Syscall) -> usize {
    arch::syscall0(syscall)
}

/// Invoke a Scarlet Native syscall with one argument.
pub fn syscall1(syscall: Syscall, arg1: usize) -> usize {
    arch::syscall1(syscall, arg1)
}

/// Invoke a Scarlet Native syscall with two arguments.
pub fn syscall2(syscall: Syscall, arg1: usize, arg2: usize) -> usize {
    arch::syscall2(syscall, arg1, arg2)
}

/// Invoke a Scarlet Native syscall with three arguments.
pub fn syscall3(syscall: Syscall, arg1: usize, arg2: usize, arg3: usize) -> usize {
    arch::syscall3(syscall, arg1, arg2, arg3)
}

/// Invoke a Scarlet Native syscall with four arguments.
pub fn syscall4(syscall: Syscall, arg1: usize, arg2: usize, arg3: usize, arg4: usize) -> usize {
    arch::syscall4(syscall, arg1, arg2, arg3, arg4)
}

/// Invoke a Scarlet Native syscall with five arguments.
pub fn syscall5(
    syscall: Syscall,
    arg1: usize,
    arg2: usize,
    arg3: usize,
    arg4: usize,
    arg5: usize,
) -> usize {
    arch::syscall5(syscall, arg1, arg2, arg3, arg4, arg5)
}

/// Invoke a Scarlet Native syscall with six arguments.
pub fn syscall6(
    syscall: Syscall,
    arg1: usize,
    arg2: usize,
    arg3: usize,
    arg4: usize,
    arg5: usize,
    arg6: usize,
) -> usize {
    arch::syscall6(syscall, arg1, arg2, arg3, arg4, arg5, arg6)
}
