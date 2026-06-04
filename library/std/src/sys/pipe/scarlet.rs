use crate::fmt;
use crate::io::{self, BorrowedCursor, IoSlice, IoSliceMut};
use crate::sys::pal::abi;

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
    pub(crate) fn duplicate_to_stdio(&self, target: usize) -> io::Result<()> {
        let source = if self.handle == target {
            Some(
                abi::handle_duplicate(self.handle)
                    .map_err(|()| io::Error::from(io::ErrorKind::Other))?,
            )
        } else {
            None
        };
        let source = source.unwrap_or(self.handle);

        let _ = abi::handle_close(target);
        let duplicated = match abi::handle_duplicate(source) {
            Ok(handle) => handle,
            Err(()) => {
                if source != self.handle {
                    let _ = abi::handle_close(source);
                }
                return Err(io::Error::from(io::ErrorKind::Other));
            }
        };
        if source != self.handle {
            let _ = abi::handle_close(source);
        }
        if duplicated == target {
            Ok(())
        } else {
            let _ = abi::handle_close(duplicated);
            Err(io::const_error!(
                io::ErrorKind::Uncategorized,
                "Scarlet stdio handle remap returned an unexpected handle"
            ))
        }
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

impl fmt::Debug for Pipe {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pipe").field("handle", &self.handle).finish()
    }
}
