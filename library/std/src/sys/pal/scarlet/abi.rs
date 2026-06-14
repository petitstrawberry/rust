//! Scarlet Native ABI syscall bindings used by the Scarlet `std` PAL.

use scarlet_sys::Syscall;
pub(crate) use scarlet_sys::{
    FILE_PERMISSION_WRITE, FILE_TYPE_DIRECTORY, FILE_TYPE_REGULAR, FILE_TYPE_SYMLINK,
    RawFileMetadata, SCTL_SOCKET_GET_READ_TIMEOUT_MS, SCTL_SOCKET_GET_WRITE_TIMEOUT_MS,
    SCTL_SOCKET_SET_NONBLOCK, SCTL_SOCKET_SET_READ_TIMEOUT_MS, SCTL_SOCKET_SET_WRITE_TIMEOUT_MS,
};

pub const SYSCALL_ERROR: usize = usize::MAX;
const SYSCALL_EAGAIN: usize = (-(11isize)) as usize;

pub const STDIN_HANDLE: usize = 0;
pub const STDOUT_HANDLE: usize = 1;
pub const STDERR_HANDLE: usize = 2;

pub const SOCKET_DOMAIN_INET4: usize = 2;
pub const SOCKET_DOMAIN_LOCAL: usize = 1;
pub const SOCKET_TYPE_STREAM: usize = 1;
pub const SOCKET_TYPE_DATAGRAM: usize = 2;
pub const SOCKET_PROTOCOL_DEFAULT: usize = 0;
pub const SOCKET_PROTOCOL_TCP: usize = 6;
pub const SOCKET_PROTOCOL_UDP: usize = 17;
pub const SOCKET_SHUTDOWN_READ: usize = 0;
pub const SOCKET_SHUTDOWN_WRITE: usize = 1;
pub const SOCKET_SHUTDOWN_BOTH: usize = 2;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Inet4SocketAddress {
    pub addr: [u8; 4],
    pub port: u16,
}

pub mod mmap {
    pub const PROT_NONE: usize = 0x0;
    pub const PROT_READ: usize = 0x1;
    pub const PROT_WRITE: usize = 0x2;

    pub const MAP_PRIVATE: usize = 0x02;
    pub const MAP_FIXED: usize = 0x10;
    pub const MAP_ANONYMOUS: usize = 0x20;
}

pub mod clone_flags {
    pub const VM: u64 = 0b00000001;
    pub const FS: u64 = 0b00000010;
    pub const FILES: u64 = 0b00000100;
    pub const THREAD: u64 = 0b00001000;
    pub const SET_TLS: u64 = 0b00010000;
}

#[inline]
fn syscall_result(ret: usize) -> Result<usize, ()> {
    if ret == SYSCALL_ERROR { Err(()) } else { Ok(ret) }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyscallError {
    Failed,
    WouldBlock,
}

#[inline]
fn stream_result(ret: usize, len: usize) -> Result<usize, SyscallError> {
    if ret == SYSCALL_EAGAIN {
        Err(SyscallError::WouldBlock)
    } else if ret == SYSCALL_ERROR || ret > len {
        Err(SyscallError::Failed)
    } else {
        Ok(ret)
    }
}

#[inline]
pub fn exit_group(code: i32) -> usize {
    scarlet_sys::syscall1(Syscall::ExitGroup, code as usize)
}

#[inline]
pub fn handle_close(handle: usize) -> Result<(), ()> {
    let ret = scarlet_sys::syscall1(Syscall::HandleClose, handle);
    if ret == SYSCALL_ERROR { Err(()) } else { Ok(()) }
}

#[inline]
pub fn handle_duplicate(handle: usize) -> Result<usize, ()> {
    syscall_result(scarlet_sys::syscall1(Syscall::HandleDuplicate, handle))
}

#[inline]
pub fn handle_control(handle: usize, command: u32, arg: usize) -> Result<usize, ()> {
    syscall_result(scarlet_sys::syscall3(Syscall::HandleControl, handle, command as usize, arg))
}

#[inline]
pub fn handle_duplicate_to(source_handle: usize, target_handle: usize) -> Result<(), ()> {
    let temporary_source_handle =
        if source_handle == target_handle { Some(handle_duplicate(source_handle)?) } else { None };
    let source_handle = temporary_source_handle.unwrap_or(source_handle);

    let _ = handle_close(target_handle);
    let duplicated_handle = match handle_duplicate(source_handle) {
        Ok(handle) => handle,
        Err(()) => {
            if let Some(temporary_source_handle) = temporary_source_handle {
                let _ = handle_close(temporary_source_handle);
            }
            return Err(());
        }
    };

    if let Some(temporary_source_handle) = temporary_source_handle {
        let _ = handle_close(temporary_source_handle);
    }

    if duplicated_handle == target_handle {
        Ok(())
    } else {
        let _ = handle_close(duplicated_handle);
        Err(())
    }
}

#[inline]
pub fn pipe() -> Result<(usize, usize), ()> {
    let mut pipefd = [0u32; 2];
    let ret = scarlet_sys::syscall2(Syscall::Pipe, pipefd.as_mut_ptr() as usize, 0);
    if ret == SYSCALL_ERROR { Err(()) } else { Ok((pipefd[0] as usize, pipefd[1] as usize)) }
}

#[inline]
pub fn getpid() -> Result<u32, ()> {
    syscall_result(scarlet_sys::syscall0(Syscall::Getpid)).map(|pid| pid as u32)
}

#[inline]
pub fn sleep(nanoseconds: u64) -> Result<(), ()> {
    let ret = scarlet_sys::syscall1(Syscall::Sleep, nanoseconds as usize);
    if ret == SYSCALL_ERROR { Err(()) } else { Ok(()) }
}

#[inline]
pub fn monotonic_time_ns() -> Result<u64, ()> {
    syscall_result(scarlet_sys::syscall0(Syscall::MonotonicTime)).map(|ns| ns as u64)
}

#[inline]
pub fn thread_yield() -> Result<(), ()> {
    let ret = scarlet_sys::syscall0(Syscall::Yield);
    if ret == SYSCALL_ERROR { Err(()) } else { Ok(()) }
}

#[inline]
pub fn get_random(data: &mut [u8]) -> Result<usize, ()> {
    let ret = scarlet_sys::syscall3(Syscall::GetRandom, data.as_mut_ptr() as usize, data.len(), 0);
    if ret == SYSCALL_ERROR || ret > data.len() { Err(()) } else { Ok(ret) }
}

#[inline]
pub fn clone_thread(
    flags: u64,
    stack_top: usize,
    entry: extern "C" fn(usize) -> !,
    entry_arg: usize,
    tls_ptr: usize,
) -> Result<u32, ()> {
    syscall_result(scarlet_sys::syscall5(
        Syscall::Clone,
        flags as usize,
        stack_top,
        entry as *const () as usize,
        entry_arg,
        tls_ptr,
    ))
    .map(|tid| tid as u32)
}

#[inline]
pub fn clone_process(flags: u64) -> Result<u32, ()> {
    syscall_result(scarlet_sys::syscall5(Syscall::Clone, flags as usize, 0, 0, 0, 0))
        .map(|pid| pid as u32)
}

#[inline]
pub fn execve(path: *const u8, argv: *const *const u8, envp: *const *const u8) -> Result<(), ()> {
    let ret =
        scarlet_sys::syscall4(Syscall::Execve, path as usize, argv as usize, envp as usize, 0);
    if ret == SYSCALL_ERROR { Err(()) } else { Ok(()) }
}

#[inline]
pub fn waitpid(pid: i32, status: &mut i32, options: i32) -> Result<i32, ()> {
    let ret = scarlet_sys::syscall3(
        Syscall::Waitpid,
        pid as usize,
        (status as *mut i32) as usize,
        options as usize,
    );
    if ret == SYSCALL_ERROR { Err(()) } else { Ok(ret as i32) }
}

#[inline]
pub fn thread_detach(tid: u32) -> Result<(), ()> {
    let ret = scarlet_sys::syscall1(Syscall::ThreadDetach, tid as usize);
    if ret == SYSCALL_ERROR { Err(()) } else { Ok(()) }
}

#[inline]
pub fn thread_exit_cleanup(
    code: i32,
    stack_mapping_base: usize,
    stack_mapping_len: usize,
    tls_mapping_base: usize,
    tls_mapping_len: usize,
) -> ! {
    scarlet_sys::syscall5(
        Syscall::ThreadExitCleanup,
        code as usize,
        stack_mapping_base,
        stack_mapping_len,
        tls_mapping_base,
        tls_mapping_len,
    );
    loop {
        core::hint::spin_loop();
    }
}

#[inline]
pub fn exit_current_thread(code: i32) -> ! {
    scarlet_sys::syscall1(Syscall::Exit, code as usize);
    loop {
        core::hint::spin_loop();
    }
}

#[inline]
pub fn stream_read(handle: usize, data: &mut [u8]) -> Result<usize, ()> {
    stream_read_detailed(handle, data).map_err(|_| ())
}

#[inline]
pub fn stream_read_detailed(handle: usize, data: &mut [u8]) -> Result<usize, SyscallError> {
    let ret =
        scarlet_sys::syscall3(Syscall::StreamRead, handle, data.as_mut_ptr() as usize, data.len());
    stream_result(ret, data.len())
}

#[inline]
pub fn stream_write(handle: usize, data: &[u8]) -> Result<usize, ()> {
    stream_write_detailed(handle, data).map_err(|_| ())
}

#[inline]
pub fn stream_write_detailed(handle: usize, data: &[u8]) -> Result<usize, SyscallError> {
    let ret =
        scarlet_sys::syscall3(Syscall::StreamWrite, handle, data.as_ptr() as usize, data.len());
    stream_result(ret, data.len())
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

#[inline]
pub fn sbrk(size: usize) -> Result<usize, ()> {
    syscall_result(scarlet_sys::syscall1(Syscall::Sbrk, size))
}

#[inline]
pub fn file_seek(handle: usize, offset: i64, whence: usize) -> Result<u64, ()> {
    syscall_result(scarlet_sys::syscall3(Syscall::FileSeek, handle, offset as usize, whence))
        .map(|position| position as u64)
}

#[inline]
pub fn file_truncate(handle: usize, length: u64) -> Result<(), ()> {
    let ret = scarlet_sys::syscall2(Syscall::FileTruncate, handle, length as usize);
    if ret == SYSCALL_ERROR { Err(()) } else { Ok(()) }
}

#[inline]
pub(crate) fn file_metadata(handle: usize, metadata: &mut RawFileMetadata) -> Result<(), ()> {
    let ret = scarlet_sys::syscall2(
        Syscall::FileMetadata,
        handle,
        (metadata as *mut RawFileMetadata) as usize,
    );
    if ret == SYSCALL_ERROR { Err(()) } else { Ok(()) }
}

#[inline]
pub fn vfs_open(path: *const u8, flags: usize, mode: usize) -> Result<usize, ()> {
    syscall_result(scarlet_sys::syscall3(Syscall::VfsOpen, path as usize, flags, mode))
}

#[inline]
pub fn vfs_remove(path: *const u8) -> Result<(), ()> {
    let ret = scarlet_sys::syscall1(Syscall::VfsRemove, path as usize);
    if ret == SYSCALL_ERROR { Err(()) } else { Ok(()) }
}

#[inline]
pub fn vfs_create_file(path: *const u8, mode: usize) -> Result<(), ()> {
    let ret = scarlet_sys::syscall2(Syscall::VfsCreateFile, path as usize, mode);
    if ret == SYSCALL_ERROR { Err(()) } else { Ok(()) }
}

#[inline]
pub fn vfs_create_directory(path: *const u8) -> Result<(), ()> {
    let ret = scarlet_sys::syscall1(Syscall::VfsCreateDirectory, path as usize);
    if ret == SYSCALL_ERROR { Err(()) } else { Ok(()) }
}

#[inline]
pub fn vfs_create_symlink(symlink_path: *const u8, target_path: *const u8) -> Result<(), ()> {
    let ret = scarlet_sys::syscall2(
        Syscall::VfsCreateSymlink,
        symlink_path as usize,
        target_path as usize,
    );
    if ret == SYSCALL_ERROR { Err(()) } else { Ok(()) }
}

#[inline]
pub fn vfs_change_directory(path: *const u8) -> Result<(), ()> {
    let ret = scarlet_sys::syscall1(Syscall::VfsChangeDirectory, path as usize);
    if ret == SYSCALL_ERROR { Err(()) } else { Ok(()) }
}

#[inline]
pub fn vfs_readlink(path: *const u8, buffer: &mut [u8]) -> Result<usize, ()> {
    let ret = scarlet_sys::syscall3(
        Syscall::VfsReadlink,
        path as usize,
        buffer.as_mut_ptr() as usize,
        buffer.len(),
    );
    if ret == SYSCALL_ERROR || ret > buffer.len() { Err(()) } else { Ok(ret) }
}

#[inline]
pub fn vfs_get_cwd_path(buffer: &mut [u8]) -> Result<usize, ()> {
    let ret =
        scarlet_sys::syscall2(Syscall::VfsGetCwdPath, buffer.as_mut_ptr() as usize, buffer.len());
    if ret == SYSCALL_ERROR || ret > buffer.len() { Err(()) } else { Ok(ret) }
}

#[inline]
pub fn vfs_rename(old_path: *const u8, new_path: *const u8) -> Result<(), ()> {
    let ret = scarlet_sys::syscall2(Syscall::VfsRename, old_path as usize, new_path as usize);
    if ret == SYSCALL_ERROR { Err(()) } else { Ok(()) }
}

#[inline]
pub fn vfs_create_hardlink(source_path: *const u8, target_path: *const u8) -> Result<(), ()> {
    let ret = scarlet_sys::syscall2(
        Syscall::VfsCreateHardlink,
        source_path as usize,
        target_path as usize,
    );
    if ret == SYSCALL_ERROR { Err(()) } else { Ok(()) }
}

#[inline]
pub(crate) fn vfs_metadata(path: *const u8, metadata: &mut RawFileMetadata) -> Result<(), ()> {
    let ret = scarlet_sys::syscall2(
        Syscall::VfsMetadata,
        path as usize,
        (metadata as *mut RawFileMetadata) as usize,
    );
    if ret == SYSCALL_ERROR { Err(()) } else { Ok(()) }
}

#[inline]
pub fn socket_create(domain: usize, socket_type: usize, protocol: usize) -> Result<usize, ()> {
    syscall_result(scarlet_sys::syscall3(Syscall::SocketCreate, domain, socket_type, protocol))
}

#[inline]
pub fn socket_bind_inet(handle: usize, address: &Inet4SocketAddress) -> Result<(), ()> {
    let ret = scarlet_sys::syscall3(
        Syscall::SocketBind,
        handle,
        (address as *const Inet4SocketAddress) as usize,
        size_of::<Inet4SocketAddress>(),
    );
    if ret == SYSCALL_ERROR { Err(()) } else { Ok(()) }
}

#[inline]
pub fn socket_connect_inet(handle: usize, address: &Inet4SocketAddress) -> Result<(), ()> {
    let ret = scarlet_sys::syscall3(
        Syscall::SocketConnect,
        handle,
        (address as *const Inet4SocketAddress) as usize,
        size_of::<Inet4SocketAddress>(),
    );
    if ret == SYSCALL_ERROR { Err(()) } else { Ok(()) }
}

#[inline]
pub fn socket_connect_local(handle: usize, path: &[u8]) -> Result<(), ()> {
    let ret =
        scarlet_sys::syscall3(Syscall::SocketConnect, handle, path.as_ptr() as usize, path.len());
    if ret == SYSCALL_ERROR { Err(()) } else { Ok(()) }
}

#[inline]
pub fn socket_listen(handle: usize, backlog: usize) -> Result<(), ()> {
    let ret = scarlet_sys::syscall2(Syscall::SocketListen, handle, backlog);
    if ret == SYSCALL_ERROR { Err(()) } else { Ok(()) }
}

#[inline]
pub fn socket_accept(handle: usize) -> Result<usize, ()> {
    syscall_result(scarlet_sys::syscall1(Syscall::SocketAccept, handle))
}

#[inline]
pub fn socket_shutdown(handle: usize, how: usize) -> Result<(), ()> {
    let ret = scarlet_sys::syscall2(Syscall::SocketShutdown, handle, how);
    if ret == SYSCALL_ERROR { Err(()) } else { Ok(()) }
}

#[inline]
pub fn socket_set_nonblocking(handle: usize, nonblocking: bool) -> Result<(), ()> {
    handle_control(handle, SCTL_SOCKET_SET_NONBLOCK, usize::from(nonblocking)).map(drop)
}

#[inline]
pub fn socket_set_read_timeout_ms(handle: usize, timeout_ms: usize) -> Result<(), ()> {
    handle_control(handle, SCTL_SOCKET_SET_READ_TIMEOUT_MS, timeout_ms).map(drop)
}

#[inline]
pub fn socket_set_write_timeout_ms(handle: usize, timeout_ms: usize) -> Result<(), ()> {
    handle_control(handle, SCTL_SOCKET_SET_WRITE_TIMEOUT_MS, timeout_ms).map(drop)
}

#[inline]
pub fn socket_read_timeout_ms(handle: usize) -> Result<usize, ()> {
    handle_control(handle, SCTL_SOCKET_GET_READ_TIMEOUT_MS, 0)
}

#[inline]
pub fn socket_write_timeout_ms(handle: usize) -> Result<usize, ()> {
    handle_control(handle, SCTL_SOCKET_GET_WRITE_TIMEOUT_MS, 0)
}

#[inline]
pub fn socket_recvfrom(handle: usize, data: &mut [u8], address: &mut [u8; 8]) -> Result<usize, ()> {
    socket_recvfrom_detailed(handle, data, address).map_err(|_| ())
}

#[inline]
pub fn socket_recvfrom_detailed(
    handle: usize,
    data: &mut [u8],
    address: &mut [u8; 8],
) -> Result<usize, SyscallError> {
    let ret = scarlet_sys::syscall4(
        Syscall::SocketRecvFrom,
        handle,
        data.as_mut_ptr() as usize,
        data.len(),
        address.as_mut_ptr() as usize,
    );
    stream_result(ret, data.len())
}

#[inline]
pub fn socket_sendto(handle: usize, data: &[u8], address: &[u8; 8]) -> Result<usize, ()> {
    let ret = scarlet_sys::syscall4(
        Syscall::SocketSendTo,
        handle,
        data.as_ptr() as usize,
        data.len(),
        address.as_ptr() as usize,
    );
    if ret == SYSCALL_ERROR || ret > data.len() { Err(()) } else { Ok(ret) }
}
