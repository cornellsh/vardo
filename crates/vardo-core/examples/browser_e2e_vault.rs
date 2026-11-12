//! Builds the vault the browser extension's end-to-end check unlocks.
//!
//! A copy of a fixture with logins saved for a local test page, written by
//! Vardo's own code so the test exercises a file Vardo produced. The password
//! is the fixture's, `demopass`.
//!
//! ```text
//! cargo run -p vardo-core --example browser_e2e_vault -- /tmp/e2e.kdbx 8765
//! ```

use vardo_core::model::FieldEdit;
use vardo_core::source::{SourceContext, VaultSource};
use vardo_core::Session;

fn main() {
    let mut args = std::env::args().skip(1);
    let out = std::path::PathBuf::from(args.next().expect("output path"));
    let port: u16 = args.next().map_or(8765, |p| p.parse().expect("port"));

    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/test_db_kdbx4_with_password_deleted_entry.kdbx");
    std::fs::copy(&fixture, &out).expect("copy fixture");
    let dir = out.parent().expect("parent");
    let ctx = SourceContext::local_only(dir);
    let mut session =
        Session::open(&VaultSource::file(&out), &ctx, "demopass", |_| {}).expect("unlock");

    let root = session.groups().uuid.clone();
    let mut add = |fields: &[(&str, &str)]| {
        let uuid = session.create_entry(&root).expect("create");
        let edits: Vec<FieldEdit> = fields
            .iter()
            .map(|(k, v)| FieldEdit {
                key: (*k).into(),
                value: Some((*v).into()),
                protected: *k == "Password",
            })
            .collect();
        session.update_entry(&uuid, &edits).expect("edit");
    };

    // Explicitly http, because the test page is served over plain http and
    // Vardo never fills an https login into an insecure page.
    let site = format!("http://127.0.0.1:{port}");
    add(&[
        ("Title", "Test site"),
        ("UserName", "e2e-user@example.com"),
        ("Password", "e2e-password-123"),
        ("URL", &site),
        (
            "otp",
            "otpauth://totp/Test:e2e?secret=GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ&issuer=Test",
        ),
    ]);
    // Saved for a different port, so it must never appear on the test page.
    add(&[
        ("Title", "Other service"),
        ("UserName", "nobody"),
        ("Password", "must-not-appear"),
        ("URL", &format!("http://127.0.0.1:{}", port + 1)),
    ]);

    session.save(&ctx).expect("save");
    println!("{}", out.display());
}
