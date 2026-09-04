//! Scarlet Native filesystem bindings.

use crate::ffi::{CString, OsString};
use crate::fs::TryLockError;
use crate::hash::Hash;
use crate::io::{self, BorrowedCursor, IoSlice, IoSliceMut, SeekFrom};
use crate::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, IntoRawFd, OwnedFd, RawFd};
use crate::path::{Path, PathBuf};
use crate::sys::pal::abi;
use crate::sys::time::{SystemTime, UNIX_EPOCH};
use crate::sys::{FromInner, IntoInner, unsupported};
use crate::time::Duration;
use crate::{fmt, str};

const O_WRONLY: usize = 0x1;
const O_RDWR: usize = 0x2;
const O_TRUNC: usize = 0x200;
const O_APPEND: usize = 0x400;

#[derive(Debug)]
pub struct File {
    handle: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct FileAttr {
    size: u64,
    file_type: FileType,
    perm: FilePermissions,
    created: SystemTime,
    modified: SystemTime,
    accessed: SystemTime,
}

#[derive(Debug)]
pub struct ReadDir {
    file: File,
    root: PathBuf,
}

#[derive(Clone, Debug)]
pub struct DirEntry {
    path: PathBuf,
    file_name: OsString,
    attr: FileAttr,
}

#[derive(Clone, Debug)]
pub struct OpenOptions {
    read: bool,
    write: bool,
    append: bool,
    truncate: bool,
    create: bool,
    create_new: bool,
}

#[derive(Copy, Clone, Debug, Default)]
pub struct FileTimes {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FilePermissions {
    readonly: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FileType {
    is_dir: bool,
    is_file: bool,
    is_symlink: bool,
}

#[derive(Debug)]
pub struct DirBuilder {}

#[repr(C)]
#[derive(Clone, Copy)]
struct RawDirEntry {
    file_id: u64,
    size: u64,
    file_type: u8,
    name_len: u8,
    _reserved: [u8; 6],
    name: [u8; 256],
}

impl FileAttr {
    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn perm(&self) -> FilePermissions {
        self.perm
    }

    pub fn file_type(&self) -> FileType {
        self.file_type
    }

    pub fn modified(&self) -> io::Result<SystemTime> {
        Ok(self.modified)
    }

    pub fn accessed(&self) -> io::Result<SystemTime> {
        Ok(self.accessed)
    }

    pub fn created(&self) -> io::Result<SystemTime> {
        Ok(self.created)
    }
}

impl FilePermissions {
    pub fn readonly(&self) -> bool {
        self.readonly
    }

    pub fn set_readonly(&mut self, readonly: bool) {
        self.readonly = readonly;
    }
}

impl FileTimes {
    pub fn set_accessed(&mut self, _t: SystemTime) {}
    pub fn set_modified(&mut self, _t: SystemTime) {}
}

impl FileType {
    pub fn is_dir(&self) -> bool {
        self.is_dir
    }

    pub fn is_file(&self) -> bool {
        self.is_file
    }

    pub fn is_symlink(&self) -> bool {
        self.is_symlink
    }
}

impl Iterator for ReadDir {
    type Item = io::Result<DirEntry>;

    fn next(&mut self) -> Option<io::Result<DirEntry>> {
        loop {
            let mut buffer = [0; size_of::<RawDirEntry>()];
            let raw = match self.file.read(&mut buffer) {
                Ok(0) => return None,
                Ok(n) if n < buffer.len() => {
                    return Some(Err(io::ErrorKind::InvalidData.into()));
                }
                Ok(_) => match RawDirEntry::parse(&buffer) {
                    Ok(raw) => raw,
                    Err(err) => return Some(Err(err)),
                },
                Err(err) => return Some(Err(err)),
            };
            let entry = raw.into_dir_entry(&self.root);
            if is_dot_or_dotdot(&entry.file_name) {
                continue;
            }
            return Some(Ok(entry));
        }
    }
}

impl DirEntry {
    pub fn path(&self) -> PathBuf {
        self.path.clone()
    }

    pub fn file_name(&self) -> OsString {
        self.file_name.clone()
    }

    pub fn metadata(&self) -> io::Result<FileAttr> {
        Ok(self.attr)
    }

    pub fn file_type(&self) -> io::Result<FileType> {
        Ok(self.attr.file_type())
    }
}

impl OpenOptions {
    pub fn new() -> OpenOptions {
        OpenOptions {
            read: false,
            write: false,
            append: false,
            truncate: false,
            create: false,
            create_new: false,
        }
    }

    pub fn read(&mut self, read: bool) {
        self.read = read;
    }

    pub fn write(&mut self, write: bool) {
        self.write = write;
    }

    pub fn append(&mut self, append: bool) {
        self.append = append;
    }

    pub fn truncate(&mut self, truncate: bool) {
        self.truncate = truncate;
    }

    pub fn create(&mut self, create: bool) {
        self.create = create;
    }

    pub fn create_new(&mut self, create_new: bool) {
        self.create_new = create_new;
    }
}

impl File {
    /// Return the borrowed Scarlet Native handle backing this file.
    pub(crate) fn as_raw_handle(&self) -> usize {
        self.handle
    }

    /// Construct a file that assumes ownership of a Scarlet Native handle.
    ///
    /// # Safety
    ///
    /// `handle` must be an exclusively owned, valid file-like handle.
    pub(crate) unsafe fn from_raw_handle(handle: usize) -> Self {
        Self { handle }
    }

    /// Consume the file and transfer ownership of its Scarlet Native handle.
    pub(crate) fn into_raw_handle(self) -> usize {
        core::mem::ManuallyDrop::new(self).handle
    }

    pub fn open(path: &Path, opts: &OpenOptions) -> io::Result<File> {
        validate_open_options(opts)?;
        let path = path_to_cstring(path)?;

        if opts.create_new {
            if let Ok(handle) = abi::vfs_open(path.as_ptr().cast(), 0, 0) {
                let _ = abi::handle_close(handle);
                return Err(io::ErrorKind::AlreadyExists.into());
            }
            abi::vfs_create_file(path.as_ptr().cast(), 0).map_err(|()| io::ErrorKind::Other)?;
        } else if opts.create {
            let _ = abi::vfs_create_file(path.as_ptr().cast(), 0);
        }

        let handle = abi::vfs_open(path.as_ptr().cast(), open_flags(opts), 0)
            .map_err(|()| io::ErrorKind::Other)?;
        Ok(File { handle })
    }

    pub fn file_attr(&self) -> io::Result<FileAttr> {
        let mut metadata = abi::RawFileMetadata::default();
        abi::file_metadata(self.handle, &mut metadata).map_err(|()| io::ErrorKind::Other)?;
        FileAttr::from_raw(metadata)
    }

    pub fn fsync(&self) -> io::Result<()> {
        Ok(())
    }

    pub fn datasync(&self) -> io::Result<()> {
        Ok(())
    }

    pub fn lock(&self) -> io::Result<()> {
        // TODO(scarlet): add Native handle/file lock operations once the kernel
        // exposes advisory locking for FileObject capabilities.
        unsupported()
    }

    pub fn lock_shared(&self) -> io::Result<()> {
        // TODO(scarlet): add Native handle/file lock operations once the kernel
        // exposes advisory locking for FileObject capabilities.
        unsupported()
    }

    pub fn try_lock(&self) -> Result<(), TryLockError> {
        Err(TryLockError::Error(io::Error::from(io::ErrorKind::Unsupported)))
    }

    pub fn try_lock_shared(&self) -> Result<(), TryLockError> {
        Err(TryLockError::Error(io::Error::from(io::ErrorKind::Unsupported)))
    }

    pub fn unlock(&self) -> io::Result<()> {
        // TODO(scarlet): add Native handle/file lock operations once the kernel
        // exposes advisory locking for FileObject capabilities.
        unsupported()
    }

    pub fn truncate(&self, size: u64) -> io::Result<()> {
        abi::file_truncate(self.handle, size).map_err(|()| io::ErrorKind::Other.into())
    }

    pub fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
        abi::stream_read(self.handle, buf).map_err(|()| io::ErrorKind::Other.into())
    }

    pub fn read_vectored(&self, bufs: &mut [IoSliceMut<'_>]) -> io::Result<usize> {
        crate::io::default_read_vectored(|buf| self.read(buf), bufs)
    }

    pub fn is_read_vectored(&self) -> bool {
        false
    }

    pub fn read_buf(&self, cursor: BorrowedCursor<'_>) -> io::Result<()> {
        crate::io::default_read_buf(|buf| self.read(buf), cursor)
    }

    pub fn write(&self, buf: &[u8]) -> io::Result<usize> {
        abi::stream_write(self.handle, buf).map_err(|()| io::ErrorKind::Other.into())
    }

    pub fn write_vectored(&self, bufs: &[IoSlice<'_>]) -> io::Result<usize> {
        crate::io::default_write_vectored(|buf| self.write(buf), bufs)
    }

    pub fn is_write_vectored(&self) -> bool {
        false
    }

    pub fn flush(&self) -> io::Result<()> {
        Ok(())
    }

    pub fn seek(&self, pos: SeekFrom) -> io::Result<u64> {
        let (offset, whence) = match pos {
            SeekFrom::Start(offset) => (offset as i64, 0),
            SeekFrom::Current(offset) => (offset, 1),
            SeekFrom::End(offset) => (offset, 2),
        };
        abi::file_seek(self.handle, offset, whence).map_err(|()| io::ErrorKind::Other.into())
    }

    pub fn size(&self) -> Option<io::Result<u64>> {
        Some(self.file_attr().map(|attr| attr.size()))
    }

    pub fn tell(&self) -> io::Result<u64> {
        self.seek(SeekFrom::Current(0))
    }

    pub fn duplicate(&self) -> io::Result<File> {
        abi::handle_duplicate(self.handle)
            .map(|handle| File { handle })
            .map_err(|()| io::ErrorKind::Other.into())
    }

    pub(crate) fn duplicate_to_stdio(&self, target: usize) -> io::Result<()> {
        abi::handle_duplicate_to(self.handle, target)
            .map_err(|()| io::Error::from(io::ErrorKind::Other))
    }

    pub fn set_permissions(&self, _perm: FilePermissions) -> io::Result<()> {
        // TODO(scarlet): add a Native FileObject permission mutation syscall
        // once VFS nodes support chmod-style updates.
        unsupported()
    }

    pub fn set_times(&self, _times: FileTimes) -> io::Result<()> {
        // TODO(scarlet): add Native FileObject timestamp mutation once VFS
        // filesystems expose timestamp setters.
        unsupported()
    }
}

impl Drop for File {
    fn drop(&mut self) {
        let _ = abi::handle_close(self.handle);
    }
}

impl AsRawFd for File {
    fn as_raw_fd(&self) -> RawFd {
        self.as_raw_handle() as RawFd
    }
}

impl AsFd for File {
    fn as_fd(&self) -> BorrowedFd<'_> {
        // SAFETY: the returned borrow cannot outlive this owning `File`.
        unsafe { BorrowedFd::borrow_raw(self.as_raw_fd()) }
    }
}

impl IntoRawFd for File {
    fn into_raw_fd(self) -> RawFd {
        self.into_raw_handle() as RawFd
    }
}

impl FromRawFd for File {
    unsafe fn from_raw_fd(raw_fd: RawFd) -> Self {
        // SAFETY: the trait contract requires an exclusively owned valid handle.
        unsafe { Self::from_raw_handle(raw_fd as usize) }
    }
}

impl IntoInner<OwnedFd> for File {
    fn into_inner(self) -> OwnedFd {
        // SAFETY: `into_raw_fd` transfers this file's unique handle ownership.
        unsafe { OwnedFd::from_raw_fd(self.into_raw_fd()) }
    }
}

impl FromInner<OwnedFd> for File {
    fn from_inner(owned_fd: OwnedFd) -> Self {
        // SAFETY: `into_raw_fd` transfers the `OwnedFd`'s unique ownership.
        unsafe { Self::from_raw_fd(owned_fd.into_raw_fd()) }
    }
}

impl DirBuilder {
    pub fn new() -> DirBuilder {
        DirBuilder {}
    }

    pub fn mkdir(&self, path: &Path) -> io::Result<()> {
        let path = path_to_cstring(path)?;
        abi::vfs_create_directory(path.as_ptr().cast()).map_err(|()| io::ErrorKind::Other.into())
    }
}

pub fn readdir(path: &Path) -> io::Result<ReadDir> {
    let mut opts = OpenOptions::new();
    opts.read(true);
    let file = File::open(path, &opts)?;
    Ok(ReadDir { file, root: path.to_path_buf() })
}

pub fn unlink(path: &Path) -> io::Result<()> {
    let path = path_to_cstring(path)?;
    abi::vfs_remove(path.as_ptr().cast()).map_err(|()| io::ErrorKind::Other.into())
}

pub fn rename(old: &Path, new: &Path) -> io::Result<()> {
    let old = path_to_cstring(old)?;
    let new = path_to_cstring(new)?;
    abi::vfs_rename(old.as_ptr().cast(), new.as_ptr().cast())
        .map_err(|()| io::ErrorKind::Other.into())
}

pub fn set_perm(_path: &Path, _perm: FilePermissions) -> io::Result<()> {
    // TODO(scarlet): add a VFS permission mutation syscall once VFS nodes
    // support chmod-style updates.
    unsupported()
}

pub fn set_times(_path: &Path, _times: FileTimes) -> io::Result<()> {
    // TODO(scarlet): add a VFS timestamp mutation syscall once filesystems
    // expose timestamp setters.
    unsupported()
}

pub fn set_times_nofollow(_path: &Path, _times: FileTimes) -> io::Result<()> {
    // TODO(scarlet): add no-follow timestamp mutation once VFS has lstat-style
    // path resolution and timestamp setters.
    unsupported()
}

pub fn rmdir(path: &Path) -> io::Result<()> {
    unlink(path)
}

pub fn remove_dir_all(path: &Path) -> io::Result<()> {
    let mut children = Vec::new();
    for child in readdir(path)? {
        let child = child?;
        if is_dot_or_dotdot(&child.file_name) {
            continue;
        }

        children.push((child.path(), child.file_type()?));
    }

    for (child_path, file_type) in children {
        if file_type.is_dir() && !file_type.is_symlink() {
            remove_dir_all(&child_path)?;
        } else {
            unlink(&child_path)?;
        }
    }

    rmdir(path)
}

pub fn exists(path: &Path) -> io::Result<bool> {
    let path = path_to_cstring(path)?;
    let mut metadata = abi::RawFileMetadata::default();
    match abi::vfs_metadata(path.as_ptr().cast(), &mut metadata) {
        Ok(()) => Ok(true),
        Err(()) => Ok(false),
    }
}

pub fn readlink(path: &Path) -> io::Result<PathBuf> {
    let path = path_to_cstring(path)?;
    let mut buffer = [0; 4096];
    let len =
        abi::vfs_readlink(path.as_ptr().cast(), &mut buffer).map_err(|()| io::ErrorKind::Other)?;
    let target = str::from_utf8(&buffer[..len]).map_err(|_| io::ErrorKind::InvalidData)?;
    Ok(PathBuf::from(target))
}

pub fn symlink(original: &Path, link: &Path) -> io::Result<()> {
    let original = path_to_cstring(original)?;
    let link = path_to_cstring(link)?;
    abi::vfs_create_symlink(link.as_ptr().cast(), original.as_ptr().cast())
        .map_err(|()| io::ErrorKind::Other.into())
}

pub fn link(src: &Path, dst: &Path) -> io::Result<()> {
    let src = path_to_cstring(src)?;
    let dst = path_to_cstring(dst)?;
    abi::vfs_create_hardlink(src.as_ptr().cast(), dst.as_ptr().cast())
        .map_err(|()| io::ErrorKind::Other.into())
}

pub fn stat(path: &Path) -> io::Result<FileAttr> {
    let path = path_to_cstring(path)?;
    let mut metadata = abi::RawFileMetadata::default();
    abi::vfs_metadata(path.as_ptr().cast(), &mut metadata).map_err(|()| io::ErrorKind::Other)?;
    FileAttr::from_raw(metadata)
}

pub fn lstat(path: &Path) -> io::Result<FileAttr> {
    stat(path)
}

pub fn canonicalize(path: &Path) -> io::Result<PathBuf> {
    Ok(if path.is_absolute() { path.to_path_buf() } else { crate::env::current_dir()?.join(path) })
}

pub fn copy(from: &Path, to: &Path) -> io::Result<u64> {
    let mut read_opts = OpenOptions::new();
    read_opts.read(true);
    let reader = File::open(from, &read_opts)?;

    let mut write_opts = OpenOptions::new();
    write_opts.write(true);
    write_opts.create(true);
    write_opts.truncate(true);
    let writer = File::open(to, &write_opts)?;

    let mut written_total = 0;
    let mut buffer = [0; 8192];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            return Ok(written_total);
        }

        write_all(&writer, &buffer[..read])?;
        written_total += read as u64;
    }
}

fn validate_open_options(opts: &OpenOptions) -> io::Result<()> {
    if !(opts.read || opts.write || opts.append) {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    if opts.truncate && !opts.write {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    if (opts.create || opts.create_new) && !(opts.write || opts.append) {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    Ok(())
}

fn open_flags(opts: &OpenOptions) -> usize {
    let mut flags = if opts.read && (opts.write || opts.append) {
        O_RDWR
    } else if opts.write || opts.append {
        O_WRONLY
    } else {
        0
    };

    if opts.append {
        flags |= O_APPEND;
    }
    if opts.truncate && !opts.create_new {
        flags |= O_TRUNC;
    }
    flags
}

fn path_to_cstring(path: &Path) -> io::Result<CString> {
    CString::new(path.as_os_str().as_encoded_bytes())
        .map_err(|_| io::ErrorKind::InvalidInput.into())
}

fn write_all(file: &File, mut buffer: &[u8]) -> io::Result<()> {
    while !buffer.is_empty() {
        match file.write(buffer)? {
            0 => return Err(io::ErrorKind::WriteZero.into()),
            written => buffer = &buffer[written..],
        }
    }
    Ok(())
}

fn is_dot_or_dotdot(name: &OsString) -> bool {
    matches!(name.as_encoded_bytes(), b"." | b"..")
}

impl RawDirEntry {
    fn parse(buffer: &[u8]) -> io::Result<Self> {
        if buffer.len() < size_of::<Self>() {
            return Err(io::ErrorKind::InvalidData.into());
        }

        // SAFETY: The buffer length is checked against the fixed ABI structure size above.
        let entry = unsafe { core::ptr::read_unaligned(buffer.as_ptr().cast::<Self>()) };
        if entry.name_len as usize > entry.name.len() {
            return Err(io::ErrorKind::InvalidData.into());
        }
        Ok(entry)
    }

    fn into_dir_entry(self, root: &Path) -> DirEntry {
        let name = str::from_utf8(&self.name[..self.name_len as usize]).unwrap_or("");
        let file_name = OsString::from(name);
        let is_dir = self.file_type == 1;
        let is_file = self.file_type == 0;
        let is_symlink = self.file_type == 2;

        DirEntry {
            path: root.join(&file_name),
            file_name,
            attr: FileAttr {
                size: self.size,
                file_type: FileType { is_dir, is_file, is_symlink },
                perm: FilePermissions { readonly: false },
                created: UNIX_EPOCH,
                modified: UNIX_EPOCH,
                accessed: UNIX_EPOCH,
            },
        }
    }
}

impl FileAttr {
    fn from_raw(raw: abi::RawFileMetadata) -> io::Result<Self> {
        Ok(Self {
            size: raw.size,
            file_type: FileType {
                is_dir: raw.file_type == abi::FILE_TYPE_DIRECTORY,
                is_file: raw.file_type == abi::FILE_TYPE_REGULAR,
                is_symlink: raw.file_type == abi::FILE_TYPE_SYMLINK,
            },
            perm: FilePermissions { readonly: raw.permissions & abi::FILE_PERMISSION_WRITE == 0 },
            created: system_time_from_secs(raw.created)?,
            modified: system_time_from_secs(raw.modified)?,
            accessed: system_time_from_secs(raw.accessed)?,
        })
    }
}

fn system_time_from_secs(seconds: u64) -> io::Result<SystemTime> {
    UNIX_EPOCH
        .checked_add_duration(&Duration::from_secs(seconds))
        .ok_or_else(|| io::ErrorKind::InvalidData.into())
}

impl fmt::Debug for RawDirEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RawDirEntry")
            .field("file_id", &self.file_id)
            .field("size", &self.size)
            .field("file_type", &self.file_type)
            .field("name_len", &self.name_len)
            .finish_non_exhaustive()
    }
}
