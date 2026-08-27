//! The desktop shell.
//!
//! Deliberately small. Everything with a decision in it lives in `vardo-core`,
//! which knows nothing about Tauri and is therefore testable; this crate holds
//! the window, the command wrappers, and the clipboard, which are the only
//! parts that genuinely need the toolkit.
//!
//! # Why the vault lives in Rust and not the WebView
//!
//! JavaScript strings are immutable, the engine may copy them during garbage
//! collection, and there is no `zeroize`. Once a password becomes a JS string
//! we permanently lose the ability to say when it stops existing in memory.
//!
//! So the unlocked vault stays in [`vardo_core::Session`], and the WebView is a
//! viewer that knows structure but not content. `reveal_field` is the single
//! deliberate exception, and it exists only because displaying a password
//! otherwise would mean drawing text outside the WebView.

#![forbid(unsafe_code)]

use std::sync::{Arc, Mutex};

use tauri::Manager;
use vardo_core::Session;

mod clipboard;
mod commands;

/// The unlocked session, or nothing.
///
/// `Arc` so a blocking command can take an owned handle and move it onto a
/// worker thread — saving does two KDF passes and must not run on the async
/// runtime's thread.
#[derive(Clone, Default)]
pub struct AppState {
    pub(crate) session: Arc<Mutex<Option<Session>>>,
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
            commands::vault_info,
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
            commands::sync_preview,
            commands::sync_apply,
            commands::totp,
            commands::drive_status,
            commands::drive_connect,
            commands::drive_disconnect,
            commands::drive_list_vaults,
            commands::drive_pick_vault,
            commands::fetch_icon,
            commands::entries_missing_icons,
            commands::drive_upload_vault,
            commands::forget_vault,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Vardo");
}
