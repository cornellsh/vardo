//! Quick access: a global shortcut opens a small search over the vault, and
//! Enter copies.
//!
//! Contract: the quick window calls the same commands as the main window and
//! never receives a secret. Copying happens in Rust and clears itself
//! (`inv:clipboard-written-in-rust`). The window is built on first use, so a
//! person who never presses the shortcut never pays for a second WebView.

use tauri::plugin::TauriPlugin;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder, Wry};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

/// Free on all three platforms by default. 1Password uses the same key.
pub const SHORTCUT: &str = "CommandOrControl+Shift+Space";

const LABEL: &str = "quick";

pub fn plugin() -> TauriPlugin<Wry> {
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                open(app);
            }
        })
        .build()
}

/// Claims the shortcut. Another app may own it already; Vardo still works
/// from its window, so that is a message rather than a failure.
pub fn register(app: &AppHandle) {
    if let Err(e) = app.global_shortcut().register(SHORTCUT) {
        eprintln!("vardo: quick access shortcut {SHORTCUT} unavailable: {e}");
    }
}

fn open(app: &AppHandle) {
    let window = match app.get_webview_window(LABEL) {
        Some(w) => w,
        None => {
            match WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html#quick".into()))
                .title("Vardo quick access")
                .inner_size(560.0, 380.0)
                .resizable(false)
                .decorations(false)
                .always_on_top(true)
                .skip_taskbar(true)
                .center()
                .build()
            {
                Ok(w) => w,
                Err(e) => {
                    eprintln!("vardo: could not open quick access: {e}");
                    return;
                }
            }
        }
    };
    let _ = window.center();
    let _ = window.show();
    let _ = window.set_focus();
    // A window that was only hidden keeps its last search. This clears it.
    let _ = window.emit("vault://quick-open", ());
}

/// Hides the quick window, and brings the main one forward when the person
/// needs it, for example to unlock.
#[tauri::command]
pub fn quick_done(app: AppHandle, reveal_main: bool) {
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.hide();
    }
    if reveal_main {
        crate::tray::reveal(&app);
    }
}
