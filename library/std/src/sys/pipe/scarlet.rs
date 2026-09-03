use crate::fmt;
use crate::io::{self, BorrowedCursor, IoSlice, IoSliceMut};
use crate::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, IntoRawFd, OwnedFd, RawFd};
use crate::sys::pal::abi;
use crate::sys::{FromInner, IntoInner};

#[derive(PartialEq, Eq)]
pub struct Pipe {
    handle: usize,
}

pub fn pipe() -> io::Result<(Pipe, Pipe)> {
    let (read_handle, write_handle) =
        abi::pipe().map_err(|()| io::Error::from(io::ErrorKind::Other))?;
    Ok((Pipe { handle: read_handle }, Pipe { handle: write_handle }))
}

impl Pipe {
    /// Return the borrowed Scarlet Native handle backing this pipe endpoint.
    pub(crate) fn as_raw_handle(&self) -> usize {
        self.handle
    }

    /// Construct a pipe endpoint that assumes ownership of a Scarlet Native handle.
    ///
    /// # Safety
    ///
    /// `handle` must be an exclusively owned, valid pipe endpoint handle.
    pub(crate) unsafe fn from_raw_handle(handle: usize) -> Self {
        Self { handle }
    }

    /// Consume the endpoint and transfer ownership of its Scarlet Native handle.
    pub(crate) fn into_raw_handle(self) -> usize {
        core::mem::ManuallyDrop::new(self).handle
    }

    pub(crate) fn duplicate_to_stdio(&self, target: usize) -> io::Result<()> {
        abi::handle_duplicate_to(self.handle, target)
            .map_err(|()| io::Error::from(io::ErrorKind::Other))
    }

    pub fn try_clone(&self) -> io::Result<Self> {
        abi::handle_duplicate(self.handle)
            .map(|handle| Self { handle })
            .map_err(|()| io::Error::from(io::ErrorKind::Other))
    }

    pub fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
        abi::stream_read(self.handle, buf).map_err(|()| io::Error::from(io::ErrorKind::Other))
    }

    pub fn read_buf(&self, cursor: BorrowedCursor<'_>) -> io::Result<()> {
        crate::io::default_read_buf(|buf| self.read(buf), cursor)
    }

    pub fn read_vectored(&self, bufs: &mut [IoSliceMut<'_>]) -> io::Result<usize> {
        crate::io::default_read_vectored(|buf| self.read(buf), bufs)
    }

    pub fn is_read_vectored(&self) -> bool {
        false
    }

    pub fn read_to_end(&self, buf: &mut Vec<u8>) -> io::Result<usize> {
        let start_len = buf.len();
        let mut chunk = [0; 1024];
        loop {
            match self.read(&mut chunk)? {
                0 => return Ok(buf.len() - start_len),
                n => buf.extend_from_slice(&chunk[..n]),
            }
        }
    }

    pub fn write(&self, buf: &[u8]) -> io::Result<usize> {
        abi::stream_write(self.handle, buf).map_err(|()| io::Error::from(io::ErrorKind::Other))
    }

    pub fn write_vectored(&self, bufs: &[IoSlice<'_>]) -> io::Result<usize> {
        crate::io::default_write_vectored(|buf| self.write(buf), bufs)
    }

    pub fn is_write_vectored(&self) -> bool {
        false
    }
}

impl Drop for Pipe {
    fn drop(&mut self) {
        let _ = abi::handle_close(self.handle);
    }
}

impl AsRawFd for Pipe {
    fn as_raw_fd(&self) -> RawFd {
        self.as_raw_handle() as RawFd
    }
}

impl AsFd for Pipe {
    fn as_fd(&self) -> BorrowedFd<'_> {
        // SAFETY: the returned borrow cannot outlive this owning pipe endpoint.
        unsafe { BorrowedFd::borrow_raw(self.as_raw_fd()) }
    }
}

impl IntoRawFd for Pipe {
    fn into_raw_fd(self) -> RawFd {
        self.into_raw_handle() as RawFd
    }
}

impl FromRawFd for Pipe {
    unsafe fn from_raw_fd(raw_fd: RawFd) -> Self {
        // SAFETY: the trait contract requires an exclusively owned valid handle.
        unsafe { Self::from_raw_handle(raw_fd as usize) }
    }
}

impl IntoInner<OwnedFd> for Pipe {
    fn into_inner(self) -> OwnedFd {
        // SAFETY: `into_raw_fd` transfers this endpoint's unique handle ownership.
        unsafe { OwnedFd::from_raw_fd(self.into_raw_fd()) }
    }
}

impl FromInner<OwnedFd> for Pipe {
    fn from_inner(owned_fd: OwnedFd) -> Self {
        // SAFETY: `into_raw_fd` transfers the `OwnedFd`'s unique ownership.
        unsafe { Self::from_raw_fd(owned_fd.into_raw_fd()) }
    }
}

impl fmt::Debug for Pipe {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pipe").field("handle", &self.handle).finish()
    }
}
