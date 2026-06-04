use super::unsupported;
use crate::ffi::{CString, OsStr, OsString};
use crate::path::{self, PathBuf};
use crate::sys::pal::abi;
use crate::{fmt, io, iter, slice, str};

const PATH_SEPARATOR: u8 = b':';

pub fn errno() -> i32 {
    0
}

pub fn error_string(_errno: i32) -> String {
    "operation successful".to_string()
}

pub fn getcwd() -> io::Result<PathBuf> {
    let mut buffer = [0; 4096];
    let len = abi::vfs_get_cwd_path(&mut buffer).map_err(|()| io::ErrorKind::Other)?;
    let cwd = str::from_utf8(&buffer[..len]).map_err(|_| io::ErrorKind::InvalidData)?;
    Ok(PathBuf::from(cwd))
}

pub fn chdir(path: &path::Path) -> io::Result<()> {
    let path = path_to_cstring(path)?;
    abi::vfs_change_directory(path.as_ptr().cast()).map_err(|()| io::ErrorKind::Other.into())
}

// This can't just be `impl Iterator` because that requires `'a` to be live on
// drop (see #146045).
pub type SplitPaths<'a> = iter::Map<
    slice::Split<'a, u8, impl FnMut(&u8) -> bool + 'static>,
    impl FnMut(&[u8]) -> PathBuf + 'static,
>;

#[define_opaque(SplitPaths)]
pub fn split_paths(unparsed: &OsStr) -> SplitPaths<'_> {
    fn is_separator(&b: &u8) -> bool {
        b == PATH_SEPARATOR
    }

    fn into_pathbuf(part: &[u8]) -> PathBuf {
        // SAFETY: `part` is split from bytes produced by `OsStr::as_encoded_bytes`.
        PathBuf::from(unsafe { OsStr::from_encoded_bytes_unchecked(part) })
    }

    unparsed.as_encoded_bytes().split(is_separator).map(into_pathbuf)
}

#[derive(Debug)]
pub struct JoinPathsError;

pub fn join_paths<I, T>(paths: I) -> Result<OsString, JoinPathsError>
where
    I: Iterator<Item = T>,
    T: AsRef<OsStr>,
{
    let mut joined = Vec::new();

    for (i, path) in paths.enumerate() {
        let path = path.as_ref().as_encoded_bytes();
        if i > 0 {
            joined.push(PATH_SEPARATOR);
        }
        if path.contains(&PATH_SEPARATOR) {
            return Err(JoinPathsError);
        }
        joined.extend_from_slice(path);
    }

    // SAFETY: `joined` is composed from `OsStr::as_encoded_bytes` segments and
    // ASCII separators, which preserves the platform encoding invariants.
    Ok(unsafe { OsString::from_encoded_bytes_unchecked(joined) })
}

impl fmt::Display for JoinPathsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "path segment contains separator `{}`", char::from(PATH_SEPARATOR))
    }
}

impl crate::error::Error for JoinPathsError {}

pub fn current_exe() -> io::Result<PathBuf> {
    unsupported()
}

pub fn temp_dir() -> PathBuf {
    PathBuf::from("/tmp")
}

pub fn home_dir() -> Option<PathBuf> {
    crate::sys::env::getenv(OsStr::new("HOME")).map(PathBuf::from)
}

pub fn exit(code: i32) -> ! {
    abi::exit_group(code);
    crate::intrinsics::abort()
}

pub fn getpid() -> u32 {
    panic!("no pids on this platform")
}

fn path_to_cstring(path: &path::Path) -> io::Result<CString> {
    CString::new(path.as_os_str().as_encoded_bytes())
        .map_err(|_| io::ErrorKind::InvalidInput.into())
}
