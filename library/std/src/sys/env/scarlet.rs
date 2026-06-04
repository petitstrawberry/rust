//! Scarlet Native environment variable storage.

use core::slice::memchr;

pub use super::common::Env;
use crate::collections::HashMap;
use crate::ffi::{CStr, OsStr, OsString, c_char};
use crate::io;
use crate::sync::Mutex;
use crate::sys::{FromInner, os_str};

static ENV: Mutex<Option<HashMap<OsString, OsString>>> = Mutex::new(None);

pub fn init(envp: *const *const c_char) {
    let mut guard = ENV.lock().unwrap();
    let map = guard.insert(HashMap::new());

    if envp.is_null() {
        return;
    }

    // SAFETY: Scarlet passes a null-terminated envp array at process startup.
    unsafe {
        let mut environ = envp;
        while !(*environ).is_null() {
            if let Some((key, value)) = parse(CStr::from_ptr(*environ).to_bytes()) {
                map.insert(key, value);
            }
            environ = environ.add(1);
        }
    }
}

pub fn env() -> Env {
    let guard = ENV.lock().unwrap();
    let env = guard.as_ref().expect("environment not initialized");
    Env::new(env.iter().map(|(key, value)| (key.clone(), value.clone())).collect())
}

pub fn getenv(key: &OsStr) -> Option<OsString> {
    ENV.lock().unwrap().as_ref()?.get(key).cloned()
}

pub unsafe fn setenv(key: &OsStr, value: &OsStr) -> io::Result<()> {
    let mut guard = ENV.lock().unwrap();
    let env = guard.get_or_insert_with(HashMap::new);
    env.insert(key.to_owned(), value.to_owned());
    Ok(())
}

pub unsafe fn unsetenv(key: &OsStr) -> io::Result<()> {
    if let Some(env) = ENV.lock().unwrap().as_mut() {
        env.remove(key);
    }
    Ok(())
}

fn parse(input: &[u8]) -> Option<(OsString, OsString)> {
    if input.is_empty() {
        return None;
    }

    let pos = memchr::memchr(b'=', &input[1..]).map(|p| p + 1)?;
    Some((
        OsString::from_inner(os_str::Buf { inner: input[..pos].to_vec() }),
        OsString::from_inner(os_str::Buf { inner: input[pos + 1..].to_vec() }),
    ))
}
