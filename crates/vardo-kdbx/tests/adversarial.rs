//! Hostile and malformed input.
//!
//! Every case here is a file that a real attacker could place where the app
//! will open it: a shared vault, a synced folder, a compromised cloud account.
//! The bar is not "produces the right answer" — it is **terminates quickly with
//! a clear error, and never panics**.

use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use vardo_kdbx::xml::{self, ParseLimits};
use vardo_kdbx::{open, open_with_limits, Credentials, Error, Limits};

fn fixture(name: &str) -> Vec<u8> {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name);
    std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn base() -> Vec<u8> {
    fixture("test_db_kdbx4_with_password_argon2id.kdbx")
}

fn creds() -> Credentials {
    Credentials::with_password("demopass")
}

/// Rewrites a `u64` entry in the header's KDF VariantDictionary, then repairs
/// the header digest so the file still looks intact.
///
/// This models a real attacker precisely, because **the header SHA-256 is
/// unkeyed**. It detects accidental corruption and nothing else — anyone can
/// recompute it. Only the HMAC is protective, and the HMAC cannot be checked
/// until after the KDF has already run. That is the entire argument for
/// `Limits`, and this helper is what proves the window is real.
fn patch_kdf_u64(bytes: &[u8], key: u8, value: u64) -> Vec<u8> {
    // VariantDictionary u64 entry: type 0x05, u32 keylen, key, u32 vallen, val
    let needle = [0x05, 0x01, 0, 0, 0, key, 0x08, 0, 0, 0];
    let at = bytes
        .windows(needle.len())
        .position(|w| w == needle)
        .unwrap_or_else(|| panic!("KDF parameter {:?} not found", key as char));

    let mut out = bytes.to_vec();
    out[at + needle.len()..at + needle.len() + 8].copy_from_slice(&value.to_le_bytes());

    // Repair the unkeyed header digest, exactly as an attacker would.
    let header_len = vardo_kdbx::OuterHeader::parse(&out).unwrap().raw.len();
    let digest = Sha256::digest(&out[..header_len]);
    out[header_len..header_len + 32].copy_from_slice(&digest);
    out
}

fn assert_fast<T>(budget: Duration, f: impl FnOnce() -> T) -> T {
    let start = Instant::now();
    let out = f();
    let took = start.elapsed();
    assert!(
        took < budget,
        "took {took:?}, over the {budget:?} budget — the bound is not being enforced"
    );
    out
}

// ---------------------------------------------------------------------------
// Pre-authentication resource exhaustion
// ---------------------------------------------------------------------------

#[test]
fn argon2_memory_bomb_is_refused_without_allocating() {
    // 2 GiB minus one: the largest value the KDBX specification permits.
    let evil = patch_kdf_u64(&base(), b'M', 0x7FFF_FFFF);
    let err = assert_fast(Duration::from_secs(2), || {
        open(&evil, &creds()).expect_err("must refuse")
    });
    assert!(
        matches!(err, Error::ResourceLimit(_)),
        "expected ResourceLimit, got {err:?}"
    );
}

#[test]
fn argon2_iteration_bomb_is_refused_without_running() {
    // Spec maximum. At 64 MiB this would take years.
    let evil = patch_kdf_u64(&base(), b'I', 0xFFFF_FFFF);
    let err = assert_fast(Duration::from_secs(2), || {
        open(&evil, &creds()).expect_err("must refuse")
    });
    assert!(matches!(err, Error::ResourceLimit(_)), "got {err:?}");
}

#[test]
fn aes_kdf_round_bomb_is_refused() {
    let aes = fixture("test_db_kdbx4_with_password_aes.kdbx");
    let evil = patch_kdf_u64(&aes, b'R', u64::MAX);
    let err = assert_fast(Duration::from_secs(2), || {
        open(&evil, &creds()).expect_err("must refuse")
    });
    assert!(matches!(err, Error::ResourceLimit(_)), "got {err:?}");
}

#[test]
fn limits_are_the_only_thing_standing_between_us_and_the_bomb() {
    // Same file, ceilings removed: it now gets as far as attempting the KDF.
    // Not run to completion — the point is that `Limits` is load-bearing and
    // not merely decorative.
    let evil = patch_kdf_u64(&base(), b'M', 0x7FFF_FFFF);
    let err = open(&evil, &creds()).unwrap_err();
    assert!(matches!(err, Error::ResourceLimit(_)));

    let mut generous = Limits::unlimited();
    generous.max_kdf_memory = 8 * 1024 * 1024; // small, so the test is cheap
    let err = open_with_limits(&evil, &creds(), generous).unwrap_err();
    assert!(matches!(err, Error::ResourceLimit(_)));
}

#[test]
fn oversized_file_is_refused_before_parsing() {
    let mut limits = Limits::default();
    limits.max_file_size = 16;
    let err = open_with_limits(&base(), &creds(), limits).unwrap_err();
    assert!(matches!(err, Error::ResourceLimit(_)));
}

// ---------------------------------------------------------------------------
// Structural corruption
// ---------------------------------------------------------------------------

#[test]
fn truncation_at_every_offset_errors_without_panicking() {
    let full = base();
    // Dense over the header and framing, sparse through the ciphertext.
    let offsets = (0..300).chain((300..full.len()).step_by(97));
    for n in offsets {
        let err = open(&full[..n], &creds());
        assert!(err.is_err(), "truncating to {n} bytes should not succeed");
    }
}

#[test]
fn single_bit_flips_are_detected_never_silently_accepted() {
    let full = base();
    // Sample across header, digest, HMAC, and ciphertext regions.
    for at in (0..full.len()).step_by(311) {
        for bit in [0u8, 3, 7] {
            let mut corrupt = full.clone();
            corrupt[at] ^= 1 << bit;
            if let Ok(doc) = open(&corrupt, &creds()) {
                // Accepting is only legitimate if the bytes were never
                // authenticated — which for KDBX4 is nothing at all.
                panic!(
                    "bit {bit} at offset {at} was accepted, yielding {} XML bytes",
                    doc.xml.len()
                );
            }
        }
    }
}

#[test]
fn garbage_and_empty_input_are_rejected() {
    for bad in [
        vec![],
        vec![0u8; 4],
        vec![0u8; 1024],
        b"not a kdbx file at all".to_vec(),
        vec![0xFF; 4096],
    ] {
        assert!(open(&bad, &creds()).is_err());
    }
}

#[test]
fn wrong_password_fails_as_authentication_not_corruption() {
    let err = open(&base(), &Credentials::with_password("wrong")).unwrap_err();
    assert!(matches!(err, Error::HeaderAuthFailed), "got {err:?}");
    // The message must not disclose which of the two causes it was.
    let msg = err.to_string();
    assert!(msg.contains("wrong credentials or corrupt file"));
}

#[test]
fn duplicate_header_fields_are_rejected() {
    // Splice a second CompressionFlags field in before end-of-header.
    let full = base();
    let header_len = vardo_kdbx::OuterHeader::parse(&full).unwrap().raw.len();
    // The end-of-header record is 9 bytes: id 0, u32 length 4, then 0D0A0D0A.
    let mut evil = full[..header_len - 9].to_vec();
    evil.extend_from_slice(&[3, 4, 0, 0, 0, 1, 0, 0, 0]); // CompressionFlags again
    evil.extend_from_slice(&[0, 4, 0, 0, 0, 0x0D, 0x0A, 0x0D, 0x0A]); // terminator
    evil.extend_from_slice(&full[header_len..]);

    let err = open(&evil, &creds()).unwrap_err();
    assert!(
        matches!(err, Error::Malformed { what, .. } if what == "outer header"),
        "got {err:?}"
    );
}

// ---------------------------------------------------------------------------
// XML-layer attacks
// ---------------------------------------------------------------------------

#[test]
fn doctype_is_rejected_outright() {
    // "Billion laughs": without DTD support this cannot even be expressed.
    let bomb = br#"<?xml version="1.0"?>
<!DOCTYPE lolz [
 <!ENTITY lol "lol">
 <!ENTITY lol2 "&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;">
]>
<lolz>&lol2;</lolz>"#;
    let err = xml::parse(bomb, ParseLimits::default()).unwrap_err();
    assert!(err.to_string().contains("DOCTYPE"), "got {err}");
}

#[test]
fn external_entity_reference_cannot_reach_the_filesystem() {
    let xxe = br#"<?xml version="1.0"?><!DOCTYPE r [<!ENTITY x SYSTEM "file:///etc/passwd">]><r>&x;</r>"#;
    assert!(xml::parse(xxe, ParseLimits::default()).is_err());

    // Even without a DTD, an undefined entity is passed through as literal
    // text and never resolved.
    let doc = xml::parse(b"<r>&x;</r>", ParseLimits::default()).unwrap();
    assert_eq!(doc.root.text(), "&x;");
}

#[test]
fn deep_nesting_is_bounded_rather_than_overflowing_the_stack() {
    let depth = 100_000;
    let mut src = Vec::new();
    for _ in 0..depth {
        src.extend_from_slice(b"<a>");
    }
    for _ in 0..depth {
        src.extend_from_slice(b"</a>");
    }
    let err = xml::parse(&src, ParseLimits::default()).unwrap_err();
    assert!(err.to_string().contains("nesting"), "got {err}");
}

#[test]
fn malformed_markup_is_rejected_cleanly() {
    let cases: &[&[u8]] = &[
        b"",                      // empty
        b"<a>",                   // unclosed
        b"</a>",                  // stray end tag
        b"<a></b>",               // mismatched
        b"<a><b></a></b>",        // improperly nested
        b"<a attr>",              // attribute without value
        b"<a attr=unquoted>",     // unquoted value
        b"<a attr=\"unterminated",// unterminated value
        b"<a/><b/>",              // two roots
        b"<!-- unterminated",     // unterminated comment
        b"<![CDATA[unterminated", // unterminated CDATA
        b"<?pi unterminated",     // unterminated PI
        b"<1nvalid/>",            // name cannot start with a digit
        b"<!ENTITY x 'y'>",       // markup declaration
    ];
    for src in cases {
        assert!(
            xml::parse(src, ParseLimits::default()).is_err(),
            "should reject: {:?}",
            String::from_utf8_lossy(src)
        );
    }
}

#[test]
fn invalid_utf8_payload_is_reported_with_an_offset() {
    let err = xml::parse(b"<r>\xff\xfe</r>", ParseLimits::default()).unwrap_err();
    assert!(matches!(err, Error::Xml { offset: 3, .. }), "got {err:?}");
}

#[test]
fn element_count_ceiling_is_enforced() {
    let mut src = b"<r>".to_vec();
    for _ in 0..1000 {
        src.extend_from_slice(b"<a/>");
    }
    src.extend_from_slice(b"</r>");

    let limits = ParseLimits {
        max_elements: 100,
        ..ParseLimits::default()
    };
    assert!(xml::parse(&src, limits).is_err());
    assert!(xml::parse(&src, ParseLimits::default()).is_ok());
}

// ---------------------------------------------------------------------------
// Protected-value edge cases
// ---------------------------------------------------------------------------

#[test]
fn corrupt_base64_in_a_protected_value_errors_rather_than_panicking() {
    let bytes = base();
    let doc = open(&bytes, &creds()).unwrap();
    let mut dom = doc.parse_xml().unwrap();

    // Find the first protected value and fill it with non-base64 bytes.
    let mut patched = false;
    for e in dom.root.children.iter_mut() {
        walk_and_break_protected(e, &mut patched);
    }
    assert!(patched, "fixture has no protected value to corrupt");

    let err = doc.protected_values(&dom).unwrap_err();
    assert!(matches!(err, Error::Malformed { what, .. } if what == "protected value"));
}

fn walk_and_break_protected(node: &mut xml::Node, done: &mut bool) {
    if *done {
        return;
    }
    if let xml::Node::Element(e) = node {
        if vardo_kdbx::protected::is_protected(e) {
            e.set_text("!!!not base64!!!");
            *done = true;
            return;
        }
        for c in e.children.iter_mut() {
            walk_and_break_protected(c, done);
        }
    }
}
