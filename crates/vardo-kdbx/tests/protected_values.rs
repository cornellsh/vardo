//! End-to-end protected-value decryption, cross-checked against KeePassXC.
//!
//! The expected values below were produced independently by `keepassxc-cli
//! show -a Password`, not by this implementation. That matters: a test that
//! only asserts self-consistency would happily pass with the keystream
//! advancing in the wrong order.

use vardo_kdbx::protected::{decrypt_all, protected_ordinals};
use vardo_kdbx::{open, Credentials};

fn fixture(name: &str) -> Vec<u8> {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name);
    std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn secrets_of(name: &str) -> Vec<String> {
    let bytes = fixture(name);
    let doc = open(&bytes, &Credentials::with_password("demopass")).expect("open");
    let x = doc.parse_xml().expect("parse xml");
    let vals = decrypt_all(&x, doc.inner.stream_cipher, &doc.inner.stream_key).expect("decrypt");
    (0..vals.len())
        .map(|i| vals.get_str(i).unwrap().to_string())
        .collect()
}

#[test]
fn aes_kdbx41_matches_keepassxc() {
    // keepassxc-cli show -a Password:
    //   "entry with no quality check"   -> hunter2
    //   "entry with named custom icon"  -> 123123
    //   "entry that was moved"          -> 123
    // The repeats are that entry's history, which shares the keystream.
    let got = secrets_of("test_db_kdbx41_with_password_aes.kdbx");
    assert_eq!(
        got,
        ["hunter2", "123123", "123123", "123123", "123", "123", "123", "123"]
    );
}

#[test]
fn chacha20_inner_stream_decrypts() {
    assert_eq!(secrets_of("test_db_kdbx4_with_password_argon2_chacha20.kdbx"), ["test"]);
}

#[test]
fn empty_protected_value_consumes_no_keystream() {
    // A `<Value Protected="True"/>` is legal. If it wrongly consumed keystream,
    // every following value would decrypt to garbage — so the second entry
    // being intact is the real assertion here.
    let got = secrets_of("test_db_kdbx4_with_password_argon2id.kdbx");
    assert_eq!(got, ["pass", ""]);
}

#[test]
fn ordinals_line_up_with_decrypted_values() {
    let bytes = fixture("test_db_kdbx41_with_password_aes.kdbx");
    let doc = open(&bytes, &Credentials::with_password("demopass")).unwrap();
    let x = doc.parse_xml().unwrap();
    let elements = protected_ordinals(&x);
    let vals = decrypt_all(&x, doc.inner.stream_cipher, &doc.inner.stream_key).unwrap();

    assert_eq!(elements.len(), vals.len());
    // In KDBX every protected field is a <Value> inside a <String>.
    assert!(elements.iter().all(|e| e.name == "Value"));
}

#[test]
fn decryption_does_not_disturb_the_document() {
    // Reading secrets must leave the DOM byte-identical, or saving after a
    // read would rewrite the file.
    let bytes = fixture("test_db_kdbx41_with_password_aes.kdbx");
    let doc = open(&bytes, &Credentials::with_password("demopass")).unwrap();
    let x = doc.parse_xml().unwrap();
    let before = x.to_bytes();
    let _ = decrypt_all(&x, doc.inner.stream_cipher, &doc.inner.stream_key).unwrap();
    assert_eq!(x.to_bytes(), before);
    assert_eq!(before, doc.xml);
}
