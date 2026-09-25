use std::fs::{File, OpenOptions, TryLockError};
use std::io;
use std::path::Path;

#[derive(Debug)]
pub struct Lock {
    _file: File,
}

impl Lock {
    pub fn new(path: &Path, wait: bool, create: bool, exclusive: bool) -> io::Result<Self> {
        let file = OpenOptions::new().read(true).write(true).create(create).open(path)?;

        if wait {
            if exclusive { file.lock() } else { file.lock_shared() }?;
        } else {
            let result = if exclusive { file.try_lock() } else { file.try_lock_shared() };
            match result {
                Ok(()) => {}
                Err(TryLockError::WouldBlock) => return Err(io::ErrorKind::WouldBlock.into()),
                Err(TryLockError::Error(error)) => return Err(error),
            }
        }

        // Scarlet advisory locks belong to the open file description; closing
        // this file releases the lock, including after a failed compilation.
        Ok(Self { _file: file })
    }

    pub fn error_unsupported(error: &io::Error) -> bool {
        error.kind() == io::ErrorKind::Unsupported
    }
}
