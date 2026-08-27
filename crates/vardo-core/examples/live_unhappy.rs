//! The unhappy paths, against a real Google Drive.
//!
//! Unit tests cover these against a fake backend, which proves the logic and
//! not the assumptions. Everything actually surprising in this project has come
//! from the real API disagreeing with what the docs implied, so the ones that
//! depend on Drive's behaviour get run for real.
//!
//! Creates its own throwaway vault and only touches that file id. Under the
//! `drive.file` scope it cannot reach anything else even by mistake.
//!
//! ```text
//! set -a; . ~/.config/vardo/oauth-client.env; set +a
//! cargo run -p vardo-core --example live_unhappy
//! ```

use std::path::PathBuf;

use vardo_core::drive_auth::DriveClient;
use vardo_core::session::{CoreError, Session};
use vardo_core::source::{SourceContext, VaultSource};

const PASSWORD: &str = "throwaway-live-test";

fn main() {
    let mut failures = 0;
    match run(&mut failures) {
        Ok(()) if failures == 0 => println!("\n\x1b[32mALL PASS\x1b[0m"),
        Ok(()) => {
            eprintln!("\n\x1b[31m{failures} check(s) failed\x1b[0m");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("\n\x1b[31mHARNESS ERROR\x1b[0m - {e}");
            std::process::exit(1);
        }
    }
}

fn check(failures: &mut i32, what: &str, ok: bool, saw: impl std::fmt::Display) {
    if ok {
        println!("  \x1b[32mok\x1b[0m   {what}");
    } else {
        *failures += 1;
        println!("  \x1b[31mFAIL\x1b[0m {what}\n        saw: {saw}");
    }
}

fn run(failures: &mut i32) -> Result<(), String> {
    let client = DriveClient {
        client_id: std::env::var("VARDO_CLIENT_ID")
            .map_err(|_| "VARDO_CLIENT_ID is not set; source oauth-client.env")?,
        client_secret: std::env::var("VARDO_CLIENT_SECRET").unwrap_or_default(),
    };
    let app_dir = PathBuf::from(std::env::var("HOME").unwrap()).join(".config/vardo");
    let ctx = SourceContext {
        app_dir: app_dir.clone(),
        client: Some(client.clone()),
    };

    println!("\x1b[1mSetting up a throwaway vault in Drive\x1b[0m");
    let local = std::env::temp_dir().join(format!("vardo-unhappy-{}.kdbx", stamp()));
    reencrypt(
        &std::fs::read(workspace().join("fixtures/test_db_kdbx4_with_password_argon2.kdbx"))
            .map_err(|e| e.to_string())?,
        "demopass",
        PASSWORD,
        &local,
    )?;
    let source = vardo_core::drive_vaults::upload(&client, &app_dir, &local)
        .map_err(|e| format!("upload: {e}"))?;
    let VaultSource::GoogleDrive { file_id, name } = &source else {
        return Err("upload did not return a Drive source".into());
    };
    println!("  {name} ({file_id})\n");

    // ---------------------------------------------------------------------
    println!("\x1b[1m1. A second device saving first must not be overwritten\x1b[0m");
    {
        let mut here = Session::open(&source, &ctx, PASSWORD, |_| {})
            .map_err(|e| format!("open here: {e}"))?;
        let group = here.groups().uuid.clone();
        let mine = here.create_entry(&group).map_err(|e| e.to_string())?;
        here.update_entry(&mine, &[field("Title", "laptop")])
            .map_err(|e| e.to_string())?;

        // The other device, opened after us, edits and saves.
        {
            let mut there = Session::open(&source, &ctx, PASSWORD, |_| {})
                .map_err(|e| format!("open there: {e}"))?;
            let g = there.groups().uuid.clone();
            let theirs = there.create_entry(&g).map_err(|e| e.to_string())?;
            there
                .update_entry(&theirs, &[field("Title", "phone")])
                .map_err(|e| e.to_string())?;
            there.save(&ctx).map_err(|e| format!("their save: {e}"))?;
        }

        let result = here.save(&ctx);
        check(
            failures,
            "a save that would clobber another device is refused",
            matches!(result, Err(CoreError::RemoteChanged { .. })),
            match &result {
                Err(e) => format!("{e:?}"),
                Ok(_) => "the save went through".into(),
            },
        );

        let after = Session::open(&source, &ctx, PASSWORD, |_| {})
            .map_err(|e| format!("reopen: {e}"))?;
        let titles: Vec<String> = after
            .list_entries(None, None)
            .into_iter()
            .map(|e| e.title)
            .collect();
        check(
            failures,
            "the other device's entry is still there",
            titles.iter().any(|t| t == "phone"),
            format!("{titles:?}"),
        );
    }

    // ---------------------------------------------------------------------
    println!("\n\x1b[1m2. Sync resolves it, and saving works afterwards\x1b[0m");
    {
        let mut here = Session::open(&source, &ctx, PASSWORD, |_| {})
            .map_err(|e| format!("open: {e}"))?;
        let group = here.groups().uuid.clone();
        let mine = here.create_entry(&group).map_err(|e| e.to_string())?;
        here.update_entry(&mine, &[field("Title", "after merge")])
            .map_err(|e| e.to_string())?;

        let applied = here.sync_apply(&app_dir, &ctx, "duplicate");
        check(
            failures,
            "sync_apply succeeds against Drive",
            applied.is_ok(),
            match &applied {
                Err(e) => format!("{e}"),
                Ok(o) => format!("{o:?}"),
            },
        );

        here.update_entry(&mine, &[field("Title", "edited after merge")])
            .map_err(|e| e.to_string())?;
        let saved = here.save(&ctx);
        check(
            failures,
            "a save right after a sync is not blocked by our own upload",
            saved.is_ok(),
            match &saved {
                Err(e) => format!("{e:?}"),
                Ok(_) => "saved".into(),
            },
        );
    }

    // ---------------------------------------------------------------------
    println!("\n\x1b[1m3. A vault deleted in Drive while open\x1b[0m");
    {
        let ghost = VaultSource::GoogleDrive {
            file_id: "definitely-not-a-real-file-id".into(),
            name: "ghost.kdbx".into(),
        };
        let opened = Session::open(&ghost, &ctx, PASSWORD, |_| {});
        check(
            failures,
            "a missing file is NotFound, not a generic failure",
            matches!(opened, Err(CoreError::NotFound { .. })),
            match &opened {
                Err(e) => format!("{e:?}"),
                Ok(_) => "it opened".into(),
            },
        );
    }

    // ---------------------------------------------------------------------
    println!("\n\x1b[1m4. Being offline is not a password problem\x1b[0m");
    {
        // Pointed at a host that cannot resolve, which is what being offline
        // looks like from inside the HTTP client.
        let bad = DriveClient {
            client_id: client.client_id.clone(),
            client_secret: client.client_secret.clone(),
        };
        let offline_ctx = SourceContext {
            app_dir: PathBuf::from("/nonexistent/vardo-offline"),
            client: Some(bad),
        };
        let opened = Session::open(&source, &offline_ctx, PASSWORD, |_| {});
        check(
            failures,
            "no stored token reports auth, never a wrong password",
            !matches!(opened, Err(CoreError::WrongPassword)),
            match &opened {
                Err(e) => format!("{e:?}"),
                Ok(_) => "it opened".into(),
            },
        );
    }

    // ---------------------------------------------------------------------
    println!("\n\x1b[1m5. A wrong password stays a wrong password\x1b[0m");
    {
        let opened = Session::open(&source, &ctx, "nope nope nope", |_| {});
        check(
            failures,
            "wrong password on a Drive vault is WrongPassword",
            matches!(opened, Err(CoreError::WrongPassword)),
            match &opened {
                Err(e) => format!("{e:?}"),
                Ok(_) => "it opened".into(),
            },
        );
    }

    println!("\n  left in Drive: {name} ({file_id})");
    Ok(())
}

fn field(key: &str, value: &str) -> vardo_core::model::FieldEdit {
    vardo_core::model::FieldEdit {
        key: key.to_string(),
        value: Some(value.to_string()),
        protected: false,
    }
}

fn reencrypt(bytes: &[u8], from: &str, to: &str, out: &PathBuf) -> Result<(), String> {
    let doc = vardo_kdbx::open(bytes, &vardo_kdbx::Credentials::with_password(from))
        .map_err(|e| e.to_string())?;
    let vault = vardo_vault::Vault::open(&doc).map_err(|e| e.to_string())?;
    let encoded = vault
        .save_with(
            &vardo_kdbx::Credentials::with_password(to),
            &mut vardo_kdbx::write::OsRng,
        )
        .map_err(|e| e.to_string())?;
    std::fs::write(out, encoded).map_err(|e| e.to_string())
}

fn stamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn workspace() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}
