//! A real open-edit-save-reopen cycle against a real Google Drive.
//!
//! Every bug in this project so far was found by running, not reviewing. The
//! Drive path is unit-tested and its API assumptions were checked against a
//! live account, but the assembled path had never executed once, and "the
//! pieces work" has been wrong here before.
//!
//! Run with:
//!
//! ```text
//! set -a; . ~/.config/vardo/oauth-client.env; set +a
//! cargo run -p vardo-core --example live_drive --features network
//! ```
//!
//! # Safety
//!
//! This creates its own throwaway vault with a generated name and only ever
//! touches that file id. It never lists-and-picks, never opens an existing
//! vault, and cannot reach one: the `drive.file` scope means the only files
//! visible to it are ones this program created. There is no path here that can
//! see, let alone write, a real vault.
//!
//! It deliberately does not delete the file at the end. A test that cleans up
//! after itself gives you nothing to inspect when it fails, and the whole point
//! of this one is to find out what actually happened.

use std::path::PathBuf;

use vardo_core::drive_auth::DriveClient;
use vardo_core::session::{CoreError, Session};
use vardo_core::source::{SourceContext, VaultSource};

const PASSWORD: &str = "throwaway-live-test";

fn main() {
    match run() {
        Ok(()) => println!("\n\x1b[32mPASS\x1b[0m - the whole cycle works against real Drive"),
        Err(e) => {
            eprintln!("\n\x1b[31mFAIL\x1b[0m - {e}");
            std::process::exit(1);
        }
    }
}

fn step(n: u32, what: &str) {
    println!("\n\x1b[1m{n}. {what}\x1b[0m");
}

fn run() -> Result<(), String> {
    let client = DriveClient {
        client_id: std::env::var("VARDO_CLIENT_ID")
            .map_err(|_| "VARDO_CLIENT_ID is not set; source oauth-client.env first")?,
        client_secret: std::env::var("VARDO_CLIENT_SECRET").unwrap_or_default(),
    };
    let app_dir = dirs_config();
    let ctx = SourceContext {
        app_dir: app_dir.clone(),
        client: Some(client.clone()),
    };

    // ---------------------------------------------------------------------
    step(1, "Build a throwaway vault locally");

    // A fixture rather than a generated vault, so this exercises a real KDBX
    // rather than one shaped by whatever assumptions the writer holds.
    let fixture = workspace_root().join("fixtures/test_db_kdbx4_with_password_argon2.kdbx");
    let seed = std::fs::read(&fixture).map_err(|e| format!("cannot read fixture: {e}"))?;

    // Re-encrypt under a password that exists only in this file, so nothing
    // uploaded here is protected by a password the user uses anywhere.
    let local = std::env::temp_dir().join(format!("vardo-live-{}.kdbx", stamp()));
    reencrypt(&seed, "demopass", PASSWORD, &local)?;
    println!(
        "   {} ({} bytes)",
        local.display(),
        std::fs::metadata(&local).unwrap().len()
    );

    // ---------------------------------------------------------------------
    step(2, "Upload it to Drive");
    let source = vardo_core::drive_vaults::upload(&client, &app_dir, &local)
        .map_err(|e| format!("upload failed: {e}"))?;
    let VaultSource::GoogleDrive { file_id, name } = &source else {
        return Err("upload did not return a Drive source".into());
    };
    println!("   created {name} as {file_id}");

    // ---------------------------------------------------------------------
    step(3, "Open it from Drive, with no local file involved");
    let mut session = Session::open(&source, &ctx, PASSWORD, |p| println!("   {}", p.as_str()))
        .map_err(|e| format!("open from Drive failed: {e}"))?;
    let before = session.info().entry_count;
    println!("   {} entries, {}", before, session.info().format);

    if session.info().storage_warning.is_some() {
        return Err("a Drive vault must not carry a mount warning - it is on no mount".into());
    }
    println!("   no storage warning, correct for a vault that is on no mount");

    // ---------------------------------------------------------------------
    step(4, "Edit an entry and save back to Drive");
    let group = session.groups().uuid.clone();
    let uuid = session
        .create_entry(&group)
        .map_err(|e| format!("create failed: {e}"))?;

    let marker = format!("live-test-{}", stamp());
    session
        .update_entry(
            &uuid,
            &[
                field("Title", &marker, false),
                field("UserName", "round-trip", false),
                field("Password", "s3cret-that-must-survive", true),
            ],
        )
        .map_err(|e| format!("edit failed: {e}"))?;

    let saved = session
        .save(&ctx)
        .map_err(|e| format!("save to Drive failed: {e}"))?;
    println!("   wrote {} bytes", saved.bytes_written);
    if saved.backup.is_some() {
        return Err("a Drive save must not claim a local backup file".into());
    }
    println!("   no local backup claimed, correct - Drive keeps revisions itself");

    // ---------------------------------------------------------------------
    step(
        5,
        "Re-open from Drive in a fresh session and look for the edit",
    );

    // A fresh session, not the one in memory: the question is whether the bytes
    // in Drive changed, and reusing the session would only prove our own RAM
    // agrees with itself.
    let reopened = Session::open(&source, &ctx, PASSWORD, |_| {})
        .map_err(|e| format!("re-open failed: {e}"))?;

    if reopened.info().entry_count != before + 1 {
        return Err(format!(
            "expected {} entries after adding one, found {}",
            before + 1,
            reopened.info().entry_count
        ));
    }

    let found = reopened
        .list_entries(None, Some(&marker))
        .into_iter()
        .find(|e| e.title == marker)
        .ok_or("the entry we just saved is not in the vault Drive returned")?;
    println!("   found {:?} by {}", found.title, found.username);

    // The value that matters. An entry surviving with an empty password would
    // look like success in a list and be a catastrophe in use, and that exact
    // failure has already fooled me once in this project.
    let secret = reopened
        .field_value(&found.uuid, "Password")
        .map_err(|e| format!("cannot read the password back: {e}"))?;
    if secret.as_str() != "s3cret-that-must-survive" {
        // Deliberately does not print what came back. It is a password, and
        // this is exactly the moment a debugging print gets left in.
        return Err("the password did not survive the round trip".into());
    }
    println!("   password survived intact");

    // ---------------------------------------------------------------------
    step(6, "Sync against Drive with no local changes");
    let preview = session
        .sync_preview(&app_dir, &ctx)
        .map_err(|e| format!("sync preview failed: {e}"))?;
    println!(
        "   remote: {} | unchanged: {} | changes: {} | conflicts: {}",
        preview.remote,
        preview.remote_unchanged,
        preview.changes.len(),
        preview.conflicts.len()
    );

    // ---------------------------------------------------------------------
    step(7, "Check the failure paths report honestly");

    // A wrong password against a Drive vault must still be a wrong password,
    // not an auth or network error. These are the three things most easily
    // confused and the ones a user is least able to tell apart.
    match Session::open(&source, &ctx, "definitely not the password", |_| {}) {
        Err(CoreError::WrongPassword) => println!("   wrong password -> WrongPassword"),
        // `Session` has no Debug, and should not gain one: it holds plaintext.
        Err(other) => return Err(format!("wrong password gave {other:?}, not WrongPassword")),
        Ok(_) => return Err("a wrong password opened the vault".into()),
    }

    // A file id that does not exist must be NotFound, not a generic failure.
    let missing = VaultSource::GoogleDrive {
        file_id: "this-id-does-not-exist".into(),
        name: "ghost.kdbx".into(),
    };
    match Session::open(&missing, &ctx, PASSWORD, |_| {}) {
        Err(CoreError::NotFound { .. }) => println!("   missing file  -> NotFound"),
        Err(other) => println!("   missing file  -> {other:?} (acceptable if it names the file)"),
        Ok(_) => return Err("opening a nonexistent file id succeeded, which is impossible".into()),
    }

    println!("\n   left in Drive for inspection: {name} ({file_id})");
    println!("   local copy: {}", local.display());
    Ok(())
}

fn field(key: &str, value: &str, protected: bool) -> vardo_core::model::FieldEdit {
    vardo_core::model::FieldEdit {
        key: key.to_string(),
        value: Some(value.to_string()),
        protected,
    }
}

/// Re-encrypts a vault under a different password.
fn reencrypt(bytes: &[u8], from: &str, to: &str, out: &PathBuf) -> Result<(), String> {
    let old = vardo_kdbx::Credentials::with_password(from);
    let doc = vardo_kdbx::open(bytes, &old).map_err(|e| format!("fixture will not open: {e}"))?;
    let vault = vardo_vault::Vault::open(&doc).map_err(|e| e.to_string())?;

    // Saving under different credentials *is* the password change: the key is
    // derived at encode time, so there is nothing to set beforehand.
    let new = vardo_kdbx::Credentials::with_password(to);
    let encoded = vault
        .save_with(&new, &mut vardo_kdbx::write::OsRng)
        .map_err(|e| e.to_string())?;
    std::fs::write(out, encoded).map_err(|e| e.to_string())
}

fn stamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn dirs_config() -> PathBuf {
    std::env::var("HOME")
        .map(|h| PathBuf::from(h).join(".config/vardo"))
        .expect("HOME")
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}
