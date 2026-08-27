//! Icons the vault already carries.

use vardo_core::entries;
use vardo_vault::Vault;

fn vault_bytes(name: &str) -> Vec<u8> {
    std::fs::read(format!("../../fixtures/{name}")).expect("fixture")
}

#[test]
fn a_custom_icon_reaches_the_view_model_as_a_data_url() {
    // This fixture really does carry an embedded PNG, which is what KeePassXC
    // writes when it downloads a favicon. Rendering it costs no network and
    // leaks nothing, which is why it has to come before any thought of
    // fetching: fetching an icon the user already has would be absurd.
    let bytes = vault_bytes("test_db_kdbx41_with_password_aes.kdbx");
    let creds = vardo_kdbx::Credentials::with_password("demopass");
    let doc = vardo_kdbx::open(&bytes, &creds).expect("open");
    let vault = Vault::open(&doc).expect("vault");

    let icons = vault.custom_icons();
    assert_eq!(icons.len(), 1, "fixture should carry one icon");

    let rows = entries::list(&vault, None, None);
    let with_icon: Vec<_> = rows.iter().filter(|r| r.icon_png.is_some()).collect();
    assert_eq!(with_icon.len(), 1, "one entry should resolve to that icon");

    let url = with_icon[0].icon_png.as_deref().unwrap();
    assert!(
        url.starts_with("data:image/png;base64,"),
        "got: {}",
        &url[..40.min(url.len())]
    );

    // Whitespace inside the base64 makes a data URL browsers refuse, and the
    // failure looks like a corrupt icon rather than stray formatting.
    let payload = url.trim_start_matches("data:image/png;base64,");
    assert!(
        !payload.chars().any(char::is_whitespace),
        "base64 must be stripped of the line breaks KDBX writers insert"
    );
}

#[test]
fn entries_without_an_icon_report_none_rather_than_an_empty_string() {
    // An empty string would render as a broken image, which looks like a bug
    // rather than like an entry that simply has no icon.
    let bytes = vault_bytes("test_db_kdbx4_with_password_argon2.kdbx");
    let creds = vardo_kdbx::Credentials::with_password("demopass");
    let doc = vardo_kdbx::open(&bytes, &creds).expect("open");
    let vault = Vault::open(&doc).expect("vault");

    for row in entries::list(&vault, None, None) {
        assert!(row.icon_png.is_none(), "{} should have no icon", row.title);
    }
}
