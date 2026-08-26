//! Save fidelity.
//!
//! The contract is `payload(save(open(f))) == payload(f)`, byte for byte —
//! *not* file equality, which is impossible and undesirable (see
//! `vardo_kdbx::write`). These tests pin both halves of that: the payload must
//! be identical, and the container must not be.

use vardo_kdbx::write::Rng;
use vardo_kdbx::{open, Credentials, Error};

fn fixture(name: &str) -> Vec<u8> {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name);
    std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn creds() -> Credentials {
    Credentials::with_password("demopass")
}

/// Deterministic counter-based RNG, so failures reproduce exactly.
struct TestRng(u64);

impl Rng for TestRng {
    fn fill(&mut self, out: &mut [u8]) -> Result<(), Error> {
        for b in out.iter_mut() {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1);
            *b = (self.0 >> 33) as u8;
        }
        Ok(())
    }
}

/// Fixtures this crate can currently both read and write.
const WRITABLE: &[&str] = &[
    "test_db_kdbx4_with_password_argon2.kdbx",
    "test_db_kdbx4_with_password_argon2id.kdbx",
    "test_db_kdbx4_with_password_argon2_chacha20.kdbx",
    "test_db_kdbx4_with_password_argon2id_chacha20.kdbx",
    "test_db_kdbx4_with_password_deleted_entry.kdbx",
    "test_db_kdbx4_with_password_aes.kdbx",
    "test_db_kdbx41_features.kdbx",
    "test_db_kdbx41_with_password_aes.kdbx",
];

/// Canonical payload: XML with protected values as plaintext. Raw XML cannot
/// be compared across saves because protected values are re-encrypted under a
/// fresh inner key every time — by design, not by accident.
fn canonical(bytes: &[u8]) -> Vec<u8> {
    let doc = open(bytes, &creds()).expect("open");
    let dom = doc.parse_xml().expect("parse");
    doc.canonical_payload(&dom).expect("canonicalise").to_vec()
}

#[test]
fn payload_survives_a_save_byte_for_byte() {
    for name in WRITABLE {
        let bytes = fixture(name);
        let doc = open(&bytes, &creds()).unwrap_or_else(|e| panic!("{name}: open: {e}"));
        let dom = doc.parse_xml().unwrap();

        let saved = doc
            .save_with(&dom, &creds(), &mut TestRng(1))
            .unwrap_or_else(|e| panic!("{name}: save: {e}"));
        let reopened = open(&saved, &creds()).unwrap_or_else(|e| panic!("{name}: reopen: {e}"));

        assert_eq!(
            canonical(&saved),
            canonical(&bytes),
            "{name}: canonical payload changed across a save"
        );
        let _ = reopened;
    }
}

#[test]
fn format_settings_are_preserved_not_upgraded() {
    // The failure mode that disqualified the `keepass` crate: silently
    // rewriting a 4.0 file as 4.1, or swapping the user's cipher.
    for name in WRITABLE {
        let bytes = fixture(name);
        let doc = open(&bytes, &creds()).unwrap();
        let dom = doc.parse_xml().unwrap();
        let saved = doc.save_with(&dom, &creds(), &mut TestRng(2)).unwrap();
        let reopened = open(&saved, &creds()).unwrap();

        assert_eq!(reopened.version(), doc.version(), "{name}: version changed");
        assert_eq!(
            reopened.outer.cipher().unwrap(),
            doc.outer.cipher().unwrap(),
            "{name}: cipher changed"
        );
        assert_eq!(
            reopened.outer.compressed().unwrap(),
            doc.outer.compressed().unwrap(),
            "{name}: compression changed"
        );
        assert_eq!(
            reopened.outer.kdf_parameters().unwrap().get("$UUID"),
            doc.outer.kdf_parameters().unwrap().get("$UUID"),
            "{name}: KDF changed"
        );
    }
}

#[test]
fn protected_values_survive_and_are_re_encrypted() {
    let bytes = fixture("test_db_kdbx41_with_password_aes.kdbx");
    let doc = open(&bytes, &creds()).unwrap();
    let dom = doc.parse_xml().unwrap();
    let before = doc.protected_values(&dom).unwrap();

    let saved = doc.save_with(&dom, &creds(), &mut TestRng(3)).unwrap();
    let reopened = open(&saved, &creds()).unwrap();
    let dom2 = reopened.parse_xml().unwrap();
    let after = reopened.protected_values(&dom2).unwrap();

    assert_eq!(before.len(), after.len());
    for i in 0..before.len() {
        assert_eq!(
            before.get_str(i).unwrap().to_string(),
            after.get_str(i).unwrap().to_string(),
            "protected value {i} changed"
        );
    }

    // The inner key must be fresh, otherwise editing a password would reuse
    // keystream and leak the XOR of the old and new secrets.
    assert_ne!(
        reopened.inner.stream_key, doc.inner.stream_key,
        "inner stream key was reused across a save"
    );
}

#[test]
fn every_save_produces_different_ciphertext() {
    // Seeds and IVs must be fresh even when nothing changed. Identical output
    // would mean a static IV, which is a real vulnerability, not a feature.
    let bytes = fixture("test_db_kdbx4_with_password_argon2id.kdbx");
    let doc = open(&bytes, &creds()).unwrap();
    let dom = doc.parse_xml().unwrap();

    let a = doc.save_with(&dom, &creds(), &mut TestRng(10)).unwrap();
    let b = doc.save_with(&dom, &creds(), &mut TestRng(20)).unwrap();
    assert_ne!(a, b, "two saves produced identical bytes");
    assert_ne!(a, bytes, "save reproduced the input exactly");

    // ...but both carry identical data.
    assert_eq!(canonical(&a), canonical(&b));
    assert_eq!(canonical(&a), canonical(&bytes));
}

#[test]
fn an_edit_is_visible_and_nothing_else_moves() {
    let bytes = fixture("test_db_kdbx41_features.kdbx");
    let doc = open(&bytes, &creds()).unwrap();
    let mut dom = doc.parse_xml().unwrap();

    let meta = dom.root.child_mut("Meta").expect("Meta");
    meta.child_mut("Generator").expect("Generator").set_text("Vardo");

    let saved = doc.save_with(&dom, &creds(), &mut TestRng(4)).unwrap();
    let reopened = open(&saved, &creds()).unwrap();
    let dom2 = reopened.parse_xml().unwrap();

    assert_eq!(
        dom2.root.child("Meta").unwrap().child("Generator").unwrap().text(),
        "Vardo"
    );

    // Everything else is untouched: canonicalise both sides and confirm the
    // only difference is the string we changed.
    let before = canonical(&bytes);
    let after = doc.canonical_payload(&dom).unwrap().to_vec();
    assert_eq!(
        String::from_utf8_lossy(&after).replace("Vardo", "KeePassXC"),
        String::from_utf8_lossy(&before),
        "an edit disturbed bytes it should not have"
    );
}

#[test]
fn saving_with_the_wrong_password_still_produces_a_readable_file() {
    // Changing the master password is a legitimate operation; the point is
    // that the new file opens with the new credentials and not the old.
    let bytes = fixture("test_db_kdbx4_with_password_argon2id.kdbx");
    let doc = open(&bytes, &creds()).unwrap();
    let dom = doc.parse_xml().unwrap();

    let new_creds = Credentials::with_password("a different password");
    let saved = doc.save_with(&dom, &new_creds, &mut TestRng(5)).unwrap();

    assert!(open(&saved, &creds()).is_err(), "old password must not work");
    let reopened = open(&saved, &new_creds).expect("new password must work");
    let dom2 = reopened.parse_xml().unwrap();
    assert_eq!(
        reopened.canonical_payload(&dom2).unwrap().to_vec(),
        doc.canonical_payload(&dom).unwrap().to_vec()
    );
}

#[test]
fn saved_files_are_accepted_by_keepassxc() {
    // Cross-implementation check: writing something only we can read would be
    // worthless. Skipped silently when keepassxc-cli is unavailable.
    let Ok(out) = std::process::Command::new("keepassxc-cli").arg("--version").output() else {
        eprintln!("keepassxc-cli not installed; skipping interop check");
        return;
    };
    assert!(out.status.success());

    let dir = std::env::temp_dir().join("vardo-interop");
    std::fs::create_dir_all(&dir).unwrap();

    for name in WRITABLE {
        let bytes = fixture(name);
        let doc = open(&bytes, &creds()).unwrap();
        let dom = doc.parse_xml().unwrap();
        let saved = doc.save_with(&dom, &creds(), &mut TestRng(6)).unwrap();

        let path = dir.join(name);
        std::fs::write(&path, &saved).unwrap();

        let out = std::process::Command::new("keepassxc-cli")
            .args(["ls", "-R", path.to_str().unwrap()])
            .env("QT_QPA_PLATFORM", "offscreen")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut c| {
                use std::io::Write;
                c.stdin.as_mut().unwrap().write_all(b"demopass\n")?;
                c.wait_with_output()
            })
            .unwrap();

        assert!(
            out.status.success(),
            "{name}: keepassxc-cli rejected our output: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}
