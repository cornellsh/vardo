//! The typed view, read against real files.
//!
//! Expected values are taken from `keepassxc-cli`, not from this
//! implementation, so a wrong-but-self-consistent reading fails here.

use vardo_kdbx::{open, Credentials};
use vardo_vault::Vault;

fn vault(name: &str, pw: &str) -> Vault {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name);
    let bytes = std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    let doc = open(&bytes, &Credentials::with_password(pw)).expect("open");
    Vault::open(&doc).expect("unlock")
}

fn rich() -> Vault {
    vault("test_db_kdbx41_with_password_aes.kdbx", "demopass")
}

#[test]
fn reads_the_group_tree() {
    let v = rich();
    let root = v.root().expect("root group");
    assert_eq!(root.name(), "Database");

    // keepassxc-cli ls -R: "Group with tags/", "Group that was moved/"
    let names: Vec<_> = root.groups().map(|g| g.name()).collect();
    assert_eq!(names, ["Group with tags", "Group that was moved"]);
}

#[test]
fn reads_entry_titles_in_document_order() {
    let v = rich();
    // keepassxc-cli ls -R lists exactly these four, in this order.
    let titles: Vec<_> = v.entries().iter().map(|e| e.title()).collect();
    assert_eq!(
        titles,
        [
            "entry with no quality check",
            "entry with named custom icon",
            "entry that was moved",
            "entry with custom data",
        ]
    );
}

#[test]
fn decrypts_passwords_matching_keepassxc() {
    let v = rich();
    let entries = v.entries();
    let by_title = |t: &str| {
        entries
            .iter()
            .find(|e| e.title() == t)
            .unwrap_or_else(|| panic!("no entry {t:?}"))
    };

    // keepassxc-cli show -a Password
    for (title, expected) in [
        ("entry with no quality check", "hunter2"),
        ("entry with named custom icon", "123123"),
        ("entry that was moved", "123"),
    ] {
        let pw = by_title(title).password().expect("has a password");
        assert!(pw.is_protected(), "{title}: password must be protected");
        assert_eq!(pw.expose(), expected, "{title}");
    }
}

#[test]
fn usernames_and_urls_are_plain_text() {
    let v = rich();
    let e = v
        .entries()
        .into_iter()
        .find(|e| e.title() == "entry with no quality check")
        .unwrap();
    assert_eq!(e.username(), "jdoe");
    let username = e.field("UserName").unwrap();
    assert!(!username.is_protected());
}

#[test]
fn a_protected_field_never_leaks_through_a_plain_accessor() {
    // `title()` and friends return empty rather than ciphertext if a writer
    // has protected the field, so a secret cannot end up in a log line.
    let v = rich();
    for e in v.entries() {
        let title = e.title();
        assert!(
            !title.contains("=="),
            "title looks like base64 ciphertext: {title:?}"
        );
    }
}

#[test]
fn reads_entry_history() {
    let v = rich();
    let e = v
        .entries()
        .into_iter()
        .find(|e| e.title() == "entry with named custom icon")
        .unwrap();
    let history = e.history();
    assert_eq!(history.len(), 2, "fixture has two prior versions");

    // History entries share the document's keystream, so decrypting them is
    // the real test that ordinals line up across nesting.
    for h in &history {
        assert_eq!(h.password().unwrap().expose(), "123123");
    }
}

#[test]
fn history_entries_do_not_appear_as_ordinary_entries() {
    // They live inside <Entry><History>, so they are never direct children of
    // a group. If they leaked in, the UI would show duplicates and merge would
    // treat old versions as live records.
    let v = rich();
    assert_eq!(v.entries().len(), 4);
}

#[test]
fn reads_timestamps() {
    let v = rich();
    let root = v.root().unwrap();
    let times = root.times();
    let created = times.created.expect("root has a creation time");
    let modified = times.last_modified.expect("root has a modification time");

    assert!(!created.is_after(modified), "created after last modified");
    let year = 1970 + created.unix_seconds() / 31_557_600;
    assert!((2020..2030).contains(&year), "implausible year {year}");
    assert!(!times.expires);
    assert_eq!(times.usage_count, Some(38));
}

#[test]
fn reads_tags() {
    let v = rich();
    let group = v
        .root()
        .unwrap()
        .groups()
        .find(|g| g.name() == "Group with tags")
        .unwrap();
    assert_eq!(group.tags(), ["a", "b", "c"]);
    assert_eq!(group.notes(), "this group has tags");
}

#[test]
fn reads_previous_parent_group() {
    // KDBX 4.1 records where a moved item came from. Merge needs it to tell a
    // move apart from a delete-and-recreate.
    let v = rich();
    let moved = v
        .entries()
        .into_iter()
        .find(|e| e.title() == "entry that was moved")
        .unwrap();
    assert!(moved.previous_parent().is_some());
}

#[test]
fn reads_deleted_objects() {
    let v = rich();
    let tombstones = v.deleted_objects();
    assert_eq!(tombstones.len(), 10, "fixture records ten deletions");
    assert!(tombstones.iter().all(|t| !t.uuid.is_nil()));
    assert!(
        tombstones.iter().all(|t| t.deleted_at.is_some()),
        "a tombstone without a time cannot be ordered against an edit"
    );
}

#[test]
fn uuids_are_unique_across_live_entries() {
    // Merge matches on UUID. A duplicate would silently collapse two entries.
    let v = rich();
    let mut seen = std::collections::HashSet::new();
    for e in v.entries() {
        let uuid = e.uuid().expect("entry has a UUID");
        assert!(seen.insert(uuid), "duplicate UUID {uuid}");
    }
}

#[test]
fn history_shares_its_entry_uuid() {
    // Prior versions carry the same UUID as the live entry; that is how
    // KeePass links them. Merge must not treat them as separate records.
    let v = rich();
    let e = v
        .entries()
        .into_iter()
        .find(|e| e.title() == "entry with named custom icon")
        .unwrap();
    for h in e.history() {
        assert_eq!(h.uuid(), e.uuid());
    }
}

#[test]
fn reads_metadata() {
    let v = rich();
    assert_eq!(v.generator(), "KeePass");
    // RecycleBinEnabled is False in this fixture, so there is no bin to honour.
    assert_eq!(v.recycle_bin_uuid(), None);
}

#[test]
fn recycle_bin_is_reported_when_enabled() {
    let v = vault("test_db_kdbx4_with_password_argon2id.kdbx", "demopass");
    // This fixture has RecycleBinEnabled=True but a nil UUID, meaning "enabled
    // but not yet created". Reporting a nil UUID would make merge treat the
    // root group as the recycle bin.
    assert_eq!(v.recycle_bin_uuid(), None);
    assert_eq!(v.generator(), "KeePassXC");
}

#[test]
fn works_across_the_whole_format_matrix() {
    for (name, pw) in [
        ("test_db_kdbx4_with_password_argon2.kdbx", "demopass"),
        ("test_db_kdbx4_with_password_argon2id.kdbx", "demopass"),
        (
            "test_db_kdbx4_with_password_argon2_chacha20.kdbx",
            "demopass",
        ),
        (
            "test_db_kdbx4_with_password_argon2_twofish.kdbx",
            "demopass",
        ),
        ("test_db_kdbx41_features.kdbx", "demopass"),
        ("test_db_with_password.kdbx", "demopass"), // KDBX 3.1
        (
            "test_db_kdbx3_with_chacha20_protected_fields.kdbx",
            "password",
        ),
    ] {
        let v = vault(name, pw);
        let root = v.root().unwrap_or_else(|| panic!("{name}: no root group"));
        let _ = root.name();
        for e in v.entries() {
            // Touching every field is the point: it exercises ordinal lookup
            // for protected values on every format variant.
            let _ = e.title();
            let _ = e.fields();
        }
    }
}

#[test]
fn kdbx31_entries_decrypt_via_the_typed_view() {
    let v = vault(
        "test_db_kdbx3_with_chacha20_protected_fields.kdbx",
        "password",
    );
    let entries = v.entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].password().unwrap().expose(), "admin");
}

#[test]
fn reading_never_disturbs_the_document() {
    // The typed view is a lens, not a copy. If reading mutated the DOM, saving
    // after a read would rewrite the file.
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/test_db_kdbx41_with_password_aes.kdbx");
    let bytes = std::fs::read(p).unwrap();
    let doc = open(&bytes, &Credentials::with_password("demopass")).unwrap();
    let v = Vault::open(&doc).unwrap();

    let before = v.document().to_bytes();
    for e in v.entries() {
        let _ = e.fields();
        let _ = e.history();
        let _ = e.attachments();
    }
    assert_eq!(v.document().to_bytes(), before);
    assert_eq!(before, doc.xml);
}
