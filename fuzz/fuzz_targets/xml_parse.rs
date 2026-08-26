#![no_main]
//! The XML parser must never panic, hang, or overflow the stack on any input.
//! It is the first thing to touch attacker-influenced data after decryption.
use libfuzzer_sys::fuzz_target;
use vardo_kdbx::xml::{parse, ParseLimits};

fuzz_target!(|data: &[u8]| {
    let _ = parse(data, ParseLimits::default());
});
