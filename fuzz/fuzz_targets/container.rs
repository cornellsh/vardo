#![no_main]
//! Whole-file parsing with a fixed password.
//!
//! Most inputs fail authentication early, which still exercises the header
//! parser, the VariantDictionary decoder and the KDF bounds checks. Limits are
//! tightened so the fuzzer is not spending its budget inside Argon2.
use libfuzzer_sys::fuzz_target;
use vardo_kdbx::{open_with_limits, Credentials, Limits};

fuzz_target!(|data: &[u8]| {
    let limits = Limits {
        max_kdf_memory: 1 << 20,
        max_kdf_iterations: 2,
        max_aes_kdf_rounds: 1024,
        max_decompressed_size: 1 << 22,
        max_file_size: 1 << 22,
        ..Limits::default()
    };
    let _ = open_with_limits(data, &Credentials::with_password("demopass"), limits);
});
