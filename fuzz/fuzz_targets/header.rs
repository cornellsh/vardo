#![no_main]
//! The outer header and its VariantDictionary, reached before any
//! authentication happens. Every length here is attacker-controlled.
use libfuzzer_sys::fuzz_target;
use vardo_kdbx::header::{OuterHeader, VariantDictionary};

fuzz_target!(|data: &[u8]| {
    if let Ok(h) = OuterHeader::parse(data) {
        // Accessors must not panic on a header that parsed but is nonsensical.
        let _ = h.cipher();
        let _ = h.compressed();
        let _ = h.master_seed();
        let _ = h.encryption_iv();
        let _ = h.kdf_parameters();
    }
    let _ = VariantDictionary::parse(data);
});
