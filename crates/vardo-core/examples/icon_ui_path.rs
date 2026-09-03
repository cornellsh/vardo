//! The icon fetch through the path the app actually takes.
//!
//! The harness that proved this worked called `favicon::fetch` directly. The
//! app calls `Session::fetch_icon`, which additionally reads the entry's URL,
//! writes the result into the vault, marks it dirty, and hands back a data URL.
//! Every one of those steps is somewhere it could fail while the tested part
//! kept working.

use std::path::PathBuf;

use vardo_core::model::FieldEdit;
use vardo_core::session::Session;
use vardo_core::source::{SourceContext, VaultSource};

fn main() {
    let dir = std::env::temp_dir().join("vardo-icon-ui");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let vault = dir.join("icons.kdbx");
    std::fs::copy(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/test_db_kdbx4_with_password_argon2.kdbx"),
        &vault,
    )
    .unwrap();

    let source = VaultSource::file(&vault);
    let ctx = SourceContext::local_only(&dir);
    let mut session = Session::open(&source, &ctx, "demopass", |_| {}).expect("unlock");

    let group = session.groups().uuid.clone();
    let uuid = session.create_entry(&group).expect("create");
    session
        .update_entry(
            &uuid,
            &[
                field("Title", "GitHub"),
                field("URL", "https://github.com/settings"),
            ],
        )
        .expect("edit");

    println!("before: icon = {:?}", has_icon(&session, &uuid));

    match session.fetch_icon(&uuid) {
        Ok(Some(url)) => {
            println!("fetched {} bytes as a data URL", url.len());
            assert!(url.starts_with("data:image/png;base64,"), "wrong prefix");
        }
        Ok(None) => {
            println!("\x1b[31mFAIL\x1b[0m no icon found for github.com");
            std::process::exit(1);
        }
        Err(e) => {
            println!("\x1b[31mFAIL\x1b[0m {e}");
            std::process::exit(1);
        }
    }

    // The part the harness never covered: it has to be in the vault, and it has
    // to survive being written and read back.
    assert!(has_icon(&session, &uuid), "not stored on the entry");
    session.save(&ctx).expect("save");

    let reopened = Session::open(&source, &ctx, "demopass", |_| {}).expect("reopen");
    let stored = has_icon(&reopened, &uuid);
    println!("after save and reopen: icon = {stored:?}");
    assert!(stored, "the icon did not survive a round trip");

    println!("\n\x1b[32mPASS\x1b[0m the app's own path works end to end");
}

fn has_icon(session: &Session, uuid: &str) -> bool {
    session
        .list_entries(None, None)
        .into_iter()
        .find(|e| e.uuid == uuid)
        .map(|e| e.icon_png.is_some())
        .unwrap_or(false)
}

fn field(key: &str, value: &str) -> FieldEdit {
    FieldEdit {
        key: key.to_string(),
        value: Some(value.to_string()),
        protected: false,
    }
}
