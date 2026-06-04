//! Scarlet Native ABI syscall bindings used by the Scarlet `std` PAL.

use scarlet_sys::Syscall;

pub const SYSCALL_ERROR: usize = usize::MAX;

pub const STDIN_HANDLE: usize = 0;
pub const STDOUT_HANDLE: usize = 1;
pub const STDERR_HANDLE: usize = 2;

pub mod mmap {
    pub const PROT_READ: usize = 0x1;
    pub const PROT_WRITE: usize = 0x2;

    pub const MAP_PRIVATE: usize = 0x02;
    pub const MAP_ANONYMOUS: usize = 0x20;
}

#[inline]
pub fn exit_group(code: i32) -> usize {
    scarlet_sys::syscall1(Syscall::ExitGroup, code as usize)
}

#[inline]
pub fn stream_read(handle: usize, data: &mut [u8]) -> Result<usize, ()> {
    let ret =
        scarlet_sys::syscall3(Syscall::StreamRead, handle, data.as_mut_ptr() as usize, data.len());
    if ret == SYSCALL_ERROR || ret > data.len() { Err(()) } else { Ok(ret) }
}

#[inline]
pub fn stream_write(handle: usize, data: &[u8]) -> Result<usize, ()> {
    let ret =
        scarlet_sys::syscall3(Syscall::StreamWrite, handle, data.as_ptr() as usize, data.len());
    if ret == SYSCALL_ERROR || ret > data.len() { Err(()) } else { Ok(ret) }
}

#[inline]
pub fn memory_map(
    handle: usize,
    addr: usize,
    length: usize,
    prot: usize,
    flags: usize,
    offset: usize,
) -> Result<usize, ()> {
    let ret = scarlet_sys::syscall6(Syscall::MemoryMap, handle, addr, length, prot, flags, offset);
    if ret == SYSCALL_ERROR { Err(()) } else { Ok(ret) }
}

#[inline]
pub fn memory_unmap(addr: usize, length: usize) -> Result<(), ()> {
    let ret = scarlet_sys::syscall2(Syscall::MemoryUnmap, addr, length);
    if ret == SYSCALL_ERROR { Err(()) } else { Ok(()) }
}
