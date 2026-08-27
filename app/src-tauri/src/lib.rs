//! The desktop shell.
//!
//! # Why the vault lives here and not in the WebView
//!
//! JavaScript strings are immutable, the engine may copy them during garbage
//! collection, and there is no `zeroize`. Once a password becomes a JS string
//! we permanently lose the ability to say when it stops existing in memory.
//!
//! So the unlocked [`Vault`] stays in Rust, and the WebView is a viewer that
//! knows structure but not content. `list_entries` and `get_entry` return
//! metadata; `copy_field` moves a secret to the clipboard without it ever
//! crossing the bridge. `reveal_field` is the single deliberate exception, and
//! it exists only because displaying a password otherwise would mean drawing
//! text outside the WebView.
//!
//! The commands themselves are in [`commands`] — see that module for why they
//! cannot live here.

#![forbid(unsafe_code)]

use std::sync::Mutex;

use serde::Serialize;
use tauri::Manager;
use vardo_vault::Vault;

mod clipboard;
mod commands;
mod generate;
mod model;
mod save;

use model::VaultInfo;

/// The unlocked vault, or nothing.
#[derive(Default)]
pub struct AppState {
    pub(crate) open: Mutex<Option<Opened>>,
}

pub struct Opened {
    pub(crate) vault: Vault,
    pub(crate) info: VaultInfo,
    /// Held for the session because saving re-derives the key, and prompting
    /// for the password on every save would be unusable. `Credentials` keeps it
    /// in a `Zeroizing` buffer, and it dies with `lock_vault`.
    pub(crate) creds: vardo_kdbx::Credentials,
    /// Set by any successful edit, cleared by a successful save. Drives the
    /// "unsaved changes" state in the UI.
    pub(crate) dirty: bool,
}

/// Errors as the UI models them.
///
/// Deliberately coarse. A caller must not be able to distinguish a wrong
/// password from a corrupt file any more finely than KDBX itself does.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum UiError {
    WrongPassword,
    NotFound { path: String },
    Corrupt { detail: String },
    Unsupported { detail: String },
    Io { detail: String },
    Locked,
}

impl From<vardo_kdbx::Error> for UiError {
    fn from(e: vardo_kdbx::Error) -> Self {
        use vardo_kdbx::Error as E;
        match e {
            // A wrong password and a tampered file are the same event here, and
            // that is a property of KDBX rather than a shortcut: the header MAC
            // is computed from the derived key, so a bad key and corrupt bytes
            // fail identically. Reporting them separately would invent a signal
            // the format deliberately withholds.
            E::HeaderAuthFailed | E::BlockAuthFailed { .. } => UiError::WrongPassword,
            E::UnsupportedVersion { major, minor } => UiError::Unsupported {
                detail: format!("KDBX {major}.{minor}"),
            },
            E::UnknownCipher(d) => UiError::Unsupported {
                detail: format!("cipher {d}"),
            },
            E::UnknownKdf(d) => UiError::Unsupported {
                detail: format!("KDF {d}"),
            },
            E::ResourceLimit(d) => UiError::Unsupported { detail: d },
            other => UiError::Corrupt {
                detail: other.to_string(),
            },
        }
    }
}

impl From<vardo_vault::EditError> for UiError {
    fn from(e: vardo_vault::EditError) -> Self {
        use vardo_vault::EditError as E;
        match e {
            E::ReadOnly => UiError::Unsupported {
                detail: "KDBX 3.1 databases are read-only in Vardo".into(),
            },
            E::NoSuchEntry(u) | E::NoSuchGroup(u) => UiError::NotFound {
                path: u.to_base64(),
            },
            other => UiError::Corrupt {
                detail: other.to_string(),
            },
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(|app| {
            app.manage(AppState::default());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::open_vault,
            commands::lock_vault,
            commands::groups,
            commands::list_entries,
            commands::get_entry,
            commands::copy_field,
            commands::reveal_field,
            commands::recent_vaults,
            commands::update_entry,
            commands::create_entry,
            commands::delete_entry,
            commands::move_entry,
            commands::save_vault,
            commands::is_dirty,
            commands::generate_password,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Vardo");
}
