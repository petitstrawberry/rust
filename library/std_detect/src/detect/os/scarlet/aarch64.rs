//! AArch64 Scarlet feature detection from the kernel's ELF auxiliary vector.

use crate::detect::{Feature, bit, cache};

unsafe extern "C" {
    fn __scarlet_getauxval(key: usize) -> usize;
}

pub(crate) fn detect_features() -> cache::Initializer {
    // This backend is first used by std's .init_array LSE constructor. The
    // Scarlet entry point publishes auxv before running that array.
    let hwcap = unsafe { __scarlet_getauxval(16) };
    features_from_hwcap(hwcap)
}

fn features_from_hwcap(hwcap: usize) -> cache::Initializer {
    let mut features = cache::Initializer::default();
    let mut set = |feature: Feature, enabled| {
        if enabled {
            features.set(feature as u32);
        }
    };
    let fp = bit::test(hwcap, 0);
    let fphp = bit::test(hwcap, 9);
    let asimdhp = bit::test(hwcap, 10);
    let asimd = fp && bit::test(hwcap, 1) && (!fphp || asimdhp);
    let sha1 = bit::test(hwcap, 5);
    let sha2 = bit::test(hwcap, 6);
    set(Feature::fp, fp);
    set(Feature::fp16, fp && fphp);
    set(Feature::asimd, asimd);
    set(Feature::pmull, asimd && bit::test(hwcap, 4));
    set(Feature::aes, asimd && bit::test(hwcap, 3) && bit::test(hwcap, 4));
    set(Feature::sha2, asimd && sha1 && sha2);
    set(Feature::crc, bit::test(hwcap, 7));
    set(Feature::lse, bit::test(hwcap, 8));
    features
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lse_and_crypto_follow_hwcap() {
        let low = features_from_hwcap(0);
        assert!(!low.test(Feature::lse as u32));
        let high = features_from_hwcap((1 << 0) | (1 << 1) | (1 << 3) | (1 << 4) | (1 << 8));
        assert!(high.test(Feature::lse as u32));
        assert!(high.test(Feature::aes as u32));
    }
}
