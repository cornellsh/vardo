//! Clipboard writes that clear themselves.
//!
//! A password manager that leaves a password on the clipboard indefinitely has
//! moved the secret from an encrypted file into the most widely-readable buffer
//! on the machine. So every write is paired with a scheduled wipe.

use std::time::Duration;

use tauri::AppHandle;
use tauri_plugin_clipboard_manager::ClipboardExt;

/// Write `value`, then clear after `seconds`.
///
/// The clear is conditional: if the clipboard no longer holds what we wrote,
/// the user has copied something else since and wiping it would destroy their
/// work. Comparing means reading the secret back into memory briefly, which is
/// a smaller cost than either clobbering an unrelated clipboard or leaving a
/// password sitting there.
pub fn copy_and_schedule_clear(app: &AppHandle, value: &str, seconds: u32) -> Result<(), String> {
    let clipboard = app.clipboard();
    clipboard
        .write_text(value.to_string())
        .map_err(|e| e.to_string())?;

    let expected = value.to_string();
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(u64::from(seconds)));
        let clipboard = app.clipboard();
        match clipboard.read_text() {
            Ok(current) if current == expected => {
                let _ = clipboard.write_text(String::new());
            }
            // Either the user copied something else, or the clipboard is
            // unreadable (some Wayland compositors refuse reads from an
            // unfocused window). Leaving it alone is the safe branch in both
            // cases; the alternative is deleting data we did not write.
            _ => {}
        }
    });

    Ok(())
}
