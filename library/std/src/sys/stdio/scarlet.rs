use crate::cmp;
use crate::io::{self, IoSlice};
use crate::sys::pal::abi;

pub struct Stdin;
pub struct Stdout;
pub struct Stderr;

impl Stdin {
    pub const fn new() -> Stdin {
        Stdin
    }
}

impl io::Read for Stdin {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        abi::stream_read(abi::STDIN_HANDLE, buf).map_err(|()| {
            io::const_error!(io::ErrorKind::Uncategorized, "Scarlet stream read failed")
        })
    }
}

impl Stdout {
    pub const fn new() -> Stdout {
        Stdout
    }
}

impl io::Write for Stdout {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        write(abi::STDOUT_HANDLE, buf)
    }

    fn write_vectored(&mut self, bufs: &[IoSlice<'_>]) -> io::Result<usize> {
        write_vectored(abi::STDOUT_HANDLE, bufs)
    }

    #[inline]
    fn is_write_vectored(&self) -> bool {
        true
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Stderr {
    pub const fn new() -> Stderr {
        Stderr
    }
}

impl io::Write for Stderr {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        write(abi::STDERR_HANDLE, buf)
    }

    fn write_vectored(&mut self, bufs: &[IoSlice<'_>]) -> io::Result<usize> {
        write_vectored(abi::STDERR_HANDLE, bufs)
    }

    #[inline]
    fn is_write_vectored(&self) -> bool {
        true
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub const STDIN_BUF_SIZE: usize = crate::sys::io::DEFAULT_BUF_SIZE;

pub fn is_ebadf(_err: &io::Error) -> bool {
    false
}

pub fn panic_output() -> Option<impl io::Write> {
    Some(Stderr::new())
}

fn write(handle: usize, buf: &[u8]) -> io::Result<usize> {
    abi::stream_write(handle, buf)
        .map_err(|()| io::const_error!(io::ErrorKind::Uncategorized, "Scarlet stream write failed"))
}

fn write_vectored(handle: usize, bufs: &[IoSlice<'_>]) -> io::Result<usize> {
    let mut total = 0;
    for buf in bufs.iter().take(cmp::min(bufs.len(), isize::MAX as usize)) {
        if buf.is_empty() {
            continue;
        }
        let written = write(handle, buf)?;
        total += written;
        if written != buf.len() {
            break;
        }
    }
    Ok(total)
}
