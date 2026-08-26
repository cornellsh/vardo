//! Format coverage matrix.
//!
//! Interop is the product requirement here — a vault written by KeePass 2.x on
//! Windows, KeePassXC on Linux, or KeePassium on iOS must all open. These tests
//! pin every combination the corpus provides so a regression in one cipher or
//! KDF cannot hide behind the others passing.

use vardo_kdbx::header::{
    CIPHER_AES256_CBC, CIPHER_CHACHA20, CIPHER_TWOFISH, KDF_AES, KDF_ARGON2D, KDF_ARGON2ID,
};
use vardo_kdbx::{open, Credentials};

/// Byte comparison with a readable failure, instead of dumping two 20 KB
/// arrays of integers into the test log.
fn assert_byte_eq(got: &[u8], want: &[u8], name: &str) {
    if got == want {
        return;
    }
    let at = got
        .iter()
        .zip(want.iter())
        .position(|(a, b)| a != b)
        .unwrap_or(got.len().min(want.len()));
    let lo = at.saturating_sub(50);
    panic!(
        "{name}: differs at byte {at} ({} got, {} want)\n  got:  ...{}...\n  want: ...{}...",
        got.len(),
        want.len(),
        String::from_utf8_lossy(&got[lo..(at + 50).min(got.len())]),
        String::from_utf8_lossy(&want[lo..(at + 50).min(want.len())]),
    );
}

fn read(name: &str, pw: &str) -> vardo_kdbx::Document {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name);
    let bytes = std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    open(&bytes, &Credentials::with_password(pw)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// (fixture, password, major, minor, cipher, kdf)
const MATRIX: &[(&str, &str, u16, u16, [u8; 16], [u8; 16])] = &[
    (
        "test_db_kdbx4_with_password_argon2.kdbx",
        "demopass",
        4,
        0,
        CIPHER_AES256_CBC,
        KDF_ARGON2D,
    ),
    (
        "test_db_kdbx4_with_password_argon2id.kdbx",
        "demopass",
        4,
        0,
        CIPHER_AES256_CBC,
        KDF_ARGON2ID,
    ),
    (
        "test_db_kdbx4_with_password_argon2_chacha20.kdbx",
        "demopass",
        4,
        0,
        CIPHER_CHACHA20,
        KDF_ARGON2D,
    ),
    (
        "test_db_kdbx4_with_password_argon2id_chacha20.kdbx",
        "demopass",
        4,
        0,
        CIPHER_CHACHA20,
        KDF_ARGON2ID,
    ),
    (
        "test_db_kdbx4_with_password_argon2_twofish.kdbx",
        "demopass",
        4,
        0,
        CIPHER_TWOFISH,
        KDF_ARGON2D,
    ),
    (
        "test_db_kdbx4_with_password_argon2id_twofish.kdbx",
        "demopass",
        4,
        0,
        CIPHER_TWOFISH,
        KDF_ARGON2ID,
    ),
    (
        "test_db_kdbx41_with_password_aes.kdbx",
        "demopass",
        4,
        1,
        CIPHER_AES256_CBC,
        KDF_AES,
    ),
    // KDBX 3.1 always uses AES-KDF, with parameters in dedicated header fields
    // rather than a VariantDictionary.
    (
        "test_db_with_password.kdbx",
        "demopass",
        3,
        1,
        CIPHER_AES256_CBC,
        KDF_AES,
    ),
    (
        "test_db_kdbx3_with_chacha20_protected_fields.kdbx",
        "password",
        3,
        1,
        CIPHER_AES256_CBC,
        KDF_AES,
    ),
];

#[test]
fn every_supported_cipher_and_kdf_combination_opens() {
    for (name, pw, major, minor, cipher, kdf) in MATRIX {
        let doc = read(name, pw);
        assert_eq!(doc.version().major, *major, "{name}: major version");
        assert_eq!(doc.version().minor, *minor, "{name}: minor version");
        assert_eq!(doc.outer.cipher().unwrap(), *cipher, "{name}: cipher");

        // KDBX 3.1 has no KdfParameters dictionary; its KDF is implicit.
        if *major >= 4 {
            let params = doc.outer.kdf_parameters().unwrap();
            let uuid = params.get_bytes("$UUID").unwrap();
            assert_eq!(uuid, kdf, "{name}: KDF");
        }
        assert!(!doc.xml.is_empty(), "{name}: empty payload");
    }
}

#[test]
fn every_fixture_parses_and_round_trips_its_xml() {
    for (name, pw, ..) in MATRIX {
        let doc = read(name, pw);
        let dom = doc
            .parse_xml()
            .unwrap_or_else(|e| panic!("{name}: parse: {e}"));
        assert_byte_eq(&dom.to_bytes(), &doc.xml, name);
        assert_eq!(dom.root.name, "KeePassFile", "{name}: unexpected root");
    }
}

#[test]
fn every_fixture_decrypts_its_protected_values() {
    for (name, pw, ..) in MATRIX {
        let doc = read(name, pw);
        let dom = doc.parse_xml().unwrap();
        doc.protected_values(&dom)
            .unwrap_or_else(|e| panic!("{name}: protected values: {e}"));
    }
}

#[test]
fn kdbx3_is_readable_but_deliberately_not_writable() {
    // Writing 3.1 would mean emitting a format with unkeyed, forgeable
    // integrity. Every client that reads 3.1 also reads 4.x, so there is no
    // compatibility argument for it.
    let doc = read("test_db_with_password.kdbx", "demopass");
    let dom = doc.parse_xml().unwrap();
    let err = doc
        .save(&dom, &Credentials::with_password("demopass"))
        .unwrap_err();
    assert!(
        matches!(err, vardo_kdbx::Error::UnsupportedVersion { major: 3, .. }),
        "got {err:?}"
    );
}

#[test]
fn kdbx3_wrong_password_does_not_leak_the_reason() {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/test_db_with_password.kdbx");
    let bytes = std::fs::read(p).unwrap();
    // In 3.1 a bad password is detected via StreamStartBytes *after*
    // decryption, and bad CBC padding must not be reported differently.
    let err = open(&bytes, &Credentials::with_password("wrong")).unwrap_err();
    assert!(matches!(err, vardo_kdbx::Error::HeaderAuthFailed), "got {err:?}");
}
