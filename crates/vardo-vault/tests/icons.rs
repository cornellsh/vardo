//! Writing an icon into the vault.

use vardo_kdbx::Credentials;
use vardo_vault::Vault;

const PW: &str = "demopass";

/// A one-pixel PNG. Real bytes, so the magic-number check is exercised.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

fn fixture() -> Vec<u8> {
    std::fs::read("../../fixtures/test_db_kdbx4_with_password_argon2.kdbx").expect("fixture")
}

#[test]
fn an_icon_survives_a_save_and_reopen() {
    // The point of writing into the vault rather than a private cache: the
    // icon travels with the file, so it is fetched once ever instead of once
    // per device, and other KeePass clients render it too.
    let bytes = fixture();
    let creds = Credentials::with_password(PW);
    let doc = vardo_kdbx::open(&bytes, &creds).unwrap();
    let mut vault = Vault::open(&doc).unwrap();

    let target = vault.entries()[0].uuid().unwrap();
    assert!(vault.custom_icons().is_empty(), "fixture starts clean");

    let icon_uuid = vault.set_custom_icon(target, PNG).expect("set icon");

    let saved = vault
        .save_with(&creds, &mut vardo_kdbx::write::OsRng)
        .expect("save");
    let doc2 = vardo_kdbx::open(&saved, &creds).expect("reopen");
    let vault2 = Vault::open(&doc2).expect("vault");

    let icons = vault2.custom_icons();
    assert_eq!(icons.len(), 1, "the icon must survive the round trip");
    assert_eq!(icons[0].uuid, icon_uuid);

    let entry = vault2
        .entries()
        .into_iter()
        .find(|e| e.uuid() == Some(target))
        .expect("entry");
    assert_eq!(
        entry.custom_icon_uuid(),
        Some(icon_uuid),
        "the entry must point at the icon it was given"
    );
}

#[test]
fn setting_an_icon_twice_does_not_leave_two_references() {
    // KDBX allows one CustomIconUUID. A duplicate element is the kind of thing
    // another client silently drops on its next save, taking the icon with it.
    let bytes = fixture();
    let creds = Credentials::with_password(PW);
    let doc = vardo_kdbx::open(&bytes, &creds).unwrap();
    let mut vault = Vault::open(&doc).unwrap();

    let target = vault.entries()[0].uuid().unwrap();
    vault.set_custom_icon(target, PNG).unwrap();
    let second = vault.set_custom_icon(target, PNG).unwrap();

    let entry = vault
        .entries()
        .into_iter()
        .find(|e| e.uuid() == Some(target))
        .unwrap();
    assert_eq!(entry.custom_icon_uuid(), Some(second), "the newer wins");

    let refs = entry.element().children_named("CustomIconUUID").count();
    assert_eq!(refs, 1, "exactly one reference, got {refs}");
}

#[test]
fn setting_an_icon_records_history_so_a_merge_cannot_discard_it() {
    // An icon appearing without a history record or a modified timestamp would
    // make the entry look untouched, and a sync could then prefer a copy that
    // never had one.
    let bytes = fixture();
    let creds = Credentials::with_password(PW);
    let doc = vardo_kdbx::open(&bytes, &creds).unwrap();
    let mut vault = Vault::open(&doc).unwrap();

    let target = vault.entries()[0].uuid().unwrap();
    let before = vault
        .entries()
        .into_iter()
        .find(|e| e.uuid() == Some(target))
        .and_then(|e| e.times().last_modified)
        .map(|t| t.unix_seconds());

    vault.set_custom_icon(target, PNG).unwrap();

    let entry = vault
        .entries()
        .into_iter()
        .find(|e| e.uuid() == Some(target))
        .unwrap();
    let after = entry.times().last_modified.map(|t| t.unix_seconds());
    assert_ne!(before, after, "the entry must be marked as modified");
    assert!(
        entry.element().child("History").is_some(),
        "a history record must be kept"
    );
}
