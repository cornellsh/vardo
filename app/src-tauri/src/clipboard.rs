//! The Tauri clipboard behind `vardo_core::clipboard`. Core decides when to
//! clear; this file only reaches the system clipboard.

use std::time::Duration;

use tauri::AppHandle;
use tauri_plugin_clipboard_manager::ClipboardExt;
use vardo_core::clipboard::{copy_then_clear, Clipboard};

struct System(AppHandle);

impl Clipboard for System {
    fn write(&self, text: &str) -> Result<(), String> {
        self.0
            .clipboard()
            .write_text(text.to_owned())
            .map_err(|e| e.to_string())
    }

    fn read(&self) -> Option<String> {
        self.0.clipboard().read_text().ok()
    }
}

/// Writes `value`, then clears it after `seconds` if it is still there.
pub fn copy_and_schedule_clear(app: &AppHandle, value: &str, seconds: u32) -> Result<(), String> {
    copy_then_clear(
        System(app.clone()),
        value,
        Duration::from_secs(u64::from(seconds)),
    )
}
