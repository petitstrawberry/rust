use crate::sys::pal::abi;

pub fn fill_bytes(bytes: &mut [u8]) {
    let mut filled = 0;
    while filled < bytes.len() {
        match abi::get_random(&mut bytes[filled..]) {
            Ok(0) => panic!("Scarlet GetRandom returned no data"),
            Ok(n) => filled += n,
            Err(()) => panic!("failed to generate random data with Scarlet GetRandom"),
        }
    }
}
