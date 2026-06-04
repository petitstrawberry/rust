use crate::sys::pal::abi;

const URANDOM_PATH: &[u8] = b"/dev/urandom\0";

pub fn fill_bytes(bytes: &mut [u8]) {
    let handle = abi::vfs_open(URANDOM_PATH.as_ptr(), 0, 0)
        .expect("failed to open /dev/urandom for Scarlet random data");
    let mut filled = 0;
    while filled < bytes.len() {
        match abi::stream_read(handle, &mut bytes[filled..]) {
            Ok(0) => panic!("/dev/urandom returned EOF"),
            Ok(n) => filled += n,
            Err(()) => {
                let _ = abi::handle_close(handle);
                panic!("failed to read random data from /dev/urandom");
            }
        }
    }
    let _ = abi::handle_close(handle);
}
