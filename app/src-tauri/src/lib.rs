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

mod browser;
mod clipboard;
mod commands;
mod quick;
mod tray;

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
        .plugin(quick::plugin())
        .setup(|app| {
            // One state, shared by the window's commands and the browser
            // extension, so both always agree on whether the vault is open.
            let state = AppState::default();
            app.manage(state.clone());
            tray::build(app.handle())?;
            quick::register(app.handle());
            browser::start(app.handle().clone(), state);
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing hides. A password manager is opened many times a day for
            // a few seconds, and quitting between uses means re-typing the
            // master password and waiting for a deliberately slow KDF each
            // time. Quit is on the tray menu, where it is unambiguous.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::open_vault,
            commands::lock_vault,
            commands::vault_info,
            commands::groups,
            commands::list_entries,
            commands::list_entries_sorted,
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
            commands::save_attachment,
            commands::health,
            commands::history,
            commands::copy_history_password,
            commands::check_totp_seed,
            commands::create_group,
            commands::rename_group,
            commands::delete_group,
            commands::group_entry_count,
            commands::set_expiry,
            commands::set_tags,
            commands::add_attachment,
            commands::remove_attachment,
            commands::entries_missing_icons,
            commands::drive_upload_vault,
            commands::forget_vault,
            commands::set_tray_unlocked,
            commands::browser_hosts,
            commands::browser_connect,
            commands::browser_disconnect,
            quick::quick_done,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Vardo");
}
