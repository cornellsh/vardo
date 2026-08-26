#![no_main]
//! Byte-stability as a fuzzed property, not just a corpus assertion.
//!
//! For ANY input the parser accepts, re-serialising must reproduce the input
//! exactly. A counterexample here is a file whose bytes we would silently
//! rewrite on save — the phantom-conflict bug, found before a user hits it.
use libfuzzer_sys::fuzz_target;
use vardo_kdbx::xml::{parse, ParseLimits};

fuzz_target!(|data: &[u8]| {
    if let Ok(doc) = parse(data, ParseLimits::default()) {
        let out = doc.to_bytes();
        assert_eq!(
            out,
            data,
            "round-trip changed bytes:\n in: {:?}\nout: {:?}",
            String::from_utf8_lossy(data),
            String::from_utf8_lossy(&out)
        );

        // Reparsing our own output must also be stable (idempotence).
        let again = parse(&out, ParseLimits::default()).expect("our output must reparse");
        assert_eq!(again.to_bytes(), out);
    }
});
