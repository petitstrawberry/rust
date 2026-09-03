//! Scarlet Native ABI definitions.
//!
//! This crate contains raw ABI definitions shared by Scarlet userland
//! libraries and Rust `std`'s Scarlet PAL. It intentionally avoids syscall
//! assembly or safe wrappers so it can stay `no_std` and dependency-free.

#![no_std]

/// Raw kernel object handle value used at the Scarlet Native ABI boundary.
pub type RawHandle = i32;

/// Raw process identifier exposed by Scarlet Native process syscalls.
pub type Pid = u32;

/// Raw thread identifier exposed by Scarlet Native thread syscalls.
pub type Tid = u32;

/// Raw regular file type value used in [`RawFileMetadata::file_type`].
pub const FILE_TYPE_REGULAR: u32 = 0;
/// Raw directory file type value used in [`RawFileMetadata::file_type`].
pub const FILE_TYPE_DIRECTORY: u32 = 1;
/// Raw symbolic link file type value used in [`RawFileMetadata::file_type`].
pub const FILE_TYPE_SYMLINK: u32 = 2;
/// Raw character device file type value used in [`RawFileMetadata::file_type`].
pub const FILE_TYPE_CHAR_DEVICE: u32 = 3;
/// Raw block device file type value used in [`RawFileMetadata::file_type`].
pub const FILE_TYPE_BLOCK_DEVICE: u32 = 4;
/// Raw pipe file type value used in [`RawFileMetadata::file_type`].
pub const FILE_TYPE_PIPE: u32 = 5;
/// Raw socket file type value used in [`RawFileMetadata::file_type`].
pub const FILE_TYPE_SOCKET: u32 = 6;
/// Raw unknown file type value used in [`RawFileMetadata::file_type`].
pub const FILE_TYPE_UNKNOWN: u32 = 7;

/// Raw read permission bit used in [`RawFileMetadata::permissions`].
pub const FILE_PERMISSION_READ: u32 = 1 << 0;
/// Raw write permission bit used in [`RawFileMetadata::permissions`].
pub const FILE_PERMISSION_WRITE: u32 = 1 << 1;
/// Raw execute permission bit used in [`RawFileMetadata::permissions`].
pub const FILE_PERMISSION_EXECUTE: u32 = 1 << 2;

/// Scarlet-private socket control opcode for setting non-blocking mode.
pub const SCTL_SOCKET_SET_NONBLOCK: u32 = 0x5353_0007;
/// Scarlet-private socket control opcode for querying non-blocking mode.
pub const SCTL_SOCKET_GET_NONBLOCK: u32 = 0x5353_000B;
/// Scarlet-private socket control opcode for setting read timeout in milliseconds.
pub const SCTL_SOCKET_SET_READ_TIMEOUT_MS: u32 = 0x5353_000C;
/// Scarlet-private socket control opcode for setting write timeout in milliseconds.
pub const SCTL_SOCKET_SET_WRITE_TIMEOUT_MS: u32 = 0x5353_000D;
/// Scarlet-private socket control opcode for querying read timeout in milliseconds.
pub const SCTL_SOCKET_GET_READ_TIMEOUT_MS: u32 = 0x5353_000E;
/// Scarlet-private socket control opcode for querying write timeout in milliseconds.
pub const SCTL_SOCKET_GET_WRITE_TIMEOUT_MS: u32 = 0x5353_000F;
/// Scarlet-private socket control opcode for taking the pending socket error.
///
/// The return value is zero when no error is pending, or a positive `ERRNO_*`
/// value. Reading the value clears it.
pub const SCTL_SOCKET_TAKE_ERROR: u32 = 0x5353_0010;

/// Interrupted system call.
pub const ERRNO_EINTR: i32 = 4;
/// Input/output error.
pub const ERRNO_EIO: i32 = 5;
/// Bad handle.
pub const ERRNO_EBADF: i32 = 9;
/// Operation would block.
pub const ERRNO_EAGAIN: i32 = 11;
/// Invalid argument.
pub const ERRNO_EINVAL: i32 = 22;
/// Message too large.
pub const ERRNO_EMSGSIZE: i32 = 90;
/// Protocol not supported.
pub const ERRNO_EPROTONOSUPPORT: i32 = 93;
/// Operation not supported.
pub const ERRNO_EOPNOTSUPP: i32 = 95;
/// Address already in use.
pub const ERRNO_EADDRINUSE: i32 = 98;
/// Address not available.
pub const ERRNO_EADDRNOTAVAIL: i32 = 99;
/// Network is unreachable.
pub const ERRNO_ENETUNREACH: i32 = 101;
/// Connection aborted.
pub const ERRNO_ECONNABORTED: i32 = 103;
/// Connection reset.
pub const ERRNO_ECONNRESET: i32 = 104;
/// Socket is already connected.
pub const ERRNO_EISCONN: i32 = 106;
/// Socket is not connected.
pub const ERRNO_ENOTCONN: i32 = 107;
/// Connection timed out.
pub const ERRNO_ETIMEDOUT: i32 = 110;
/// Connection refused.
pub const ERRNO_ECONNREFUSED: i32 = 111;

/// Fixed-layout file metadata returned by Scarlet Native metadata syscalls.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RawFileMetadata {
    /// File size in bytes.
    pub size: u64,
    /// File type encoded as one of the `FILE_TYPE_*` constants.
    pub file_type: u32,
    /// Permission bits encoded as `FILE_PERMISSION_*` flags.
    pub permissions: u32,
    /// Creation timestamp in seconds.
    pub created: u64,
    /// Last modification timestamp in seconds.
    pub modified: u64,
    /// Last access timestamp in seconds.
    pub accessed: u64,
    /// Filesystem-local stable file identifier.
    pub file_id: u64,
    /// Number of hard links to this file.
    pub link_count: u32,
    /// Reserved for future ABI expansion.
    pub _reserved: u32,
}

/// Scarlet Native syscall numbers.
#[repr(usize)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Syscall {
    Invalid = 0,
    Exit = 1,
    Clone = 2,
    Execve = 3,
    ExecveABI = 4,
    Waitpid = 5,
    Kill = 6,
    Getpid = 7,
    Getppid = 8,
    Brk = 12,
    Sbrk = 13,

    // Basic I/O
    Putchar = 16,
    Getchar = 17,

    Sleep = 20,
    Yield = 21,
    GetRandom = 22,
    ExitGroup = 23,
    MonotonicTime = 35,
    SystemTime = 37,
    FutexWait = 49,
    FutexWake = 50,

    // Process information
    GetTaskInfoCount = 24,
    GetTaskInfoList = 25,
    CreateSession = 26,
    GetSessionId = 27,
    GetProcessGroupId = 28,
    SetProcessGroup = 29,

    // TLS management
    SetTls = 30,
    GetTls = 31,
    SetTidAddress = 32,
    ThreadDetach = 33,
    ThreadExitCleanup = 34,

    // ABI zone management
    RegisterAbiZone = 90,
    UnregisterAbiZone = 91,

    // Namespace management
    CreateNamespace = 92,

    // Handle management
    HandleQuery = 100,
    HandleSetRole = 101,
    HandleClose = 102,
    HandleDuplicate = 103,
    HandleControl = 110,

    // Core capabilities
    StreamRead = 200,
    StreamWrite = 201,
    Poll = 202,

    // FileObject capability
    FileSeek = 300,
    FileTruncate = 301,
    FileMetadata = 302,

    // VFS operations
    VfsOpen = 400,
    VfsRemove = 401,
    VfsCreateFile = 402,
    VfsCreateDirectory = 403,
    VfsChangeDirectory = 404,
    VfsTruncate = 405,
    VfsCreateSymlink = 406,
    VfsReadlink = 407,
    VfsGetCwdPath = 408,
    VfsRename = 409,
    VfsMetadata = 410,
    VfsCreateHardlink = 411,

    // Filesystem operations
    FsMount = 500,
    FsUmount = 501,
    FsPivotRoot = 502,

    // IPC operations
    Pipe = 600,
    EventSendDirect = 615,
    EventSendGroup = 616,

    // Shared memory
    SharedMemoryCreate = 620,
    SharedMemoryResize = 621,

    // Socket handle transfer
    SocketSendHandle = 630,
    SocketRecvHandle = 631,
    SocketSendHandleAndData = 632,
    SocketRecvHandleAndData = 633,

    // Scarlet Native event handling
    EventHandlerRegister = 640,
    EventHandlerUnregister = 641,
    EventMask = 642,
    EventReturn = 643,

    // Memory mapping operations
    MemoryMap = 700,
    MemoryUnmap = 701,

    // Socket operations
    SocketCreate = 900,
    SocketBind = 901,
    SocketListen = 902,
    SocketConnect = 903,
    SocketAccept = 904,
    Socketpair = 905,
    SocketShutdown = 906,

    // Datagram operations
    SocketRecvFrom = 907,
    SocketSendTo = 908,

    // Network configuration
    NetworkSetIpv4 = 910,
    NetworkSetGateway = 911,
    NetworkSetDns = 912,
    NetworkSetNetmask = 913,
    NetworkListInterfaces = 914,
    NetworkConfigureIpv4 = 915,
    NetworkListInterfacesV2 = 916,
    NetworkClearIpv4 = 917,
    /// Write the socket's local IPv4 address to an eight-byte native address buffer.
    SocketGetLocalAddress = 918,
    /// Write the socket's peer IPv4 address to an eight-byte native address buffer.
    SocketGetPeerAddress = 919,

    // Debug/profiler operations
    ProfilerDump = 999,

    // System control operations
    Shutdown = 1000,

    // Hypervisor operations
    ShvVmCreate = 1100,
    ShvVcpuCreate = 1101,
    ShvVcpuRun = 1102,

    // Loadable module operations
    LsmLoad = 1200,
    LsmUnload = 1201,
    LsmList = 1202,
}
