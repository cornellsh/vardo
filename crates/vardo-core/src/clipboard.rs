//! Clipboard writes that clear themselves.
//!
//! Contract: every write schedules a wipe after `CLIPBOARD_CLEAR_SECONDS`, and
//! the wipe clears only if the clipboard still holds the value
//! (`inv:clipboard-written-in-rust`).
//!
//! A password left on the clipboard has moved from an encrypted file into the
//! most widely readable buffer on the machine, so every write is paired with a
//! scheduled wipe. The shell supplies the clipboard; this module decides.

use std::time::Duration;

/// The system clipboard, as the shell reaches it.
pub trait Clipboard: Send + 'static {
    fn write(&self, text: &str) -> Result<(), String>;
    /// `None` when the clipboard cannot be read. Some Wayland compositors
    /// refuse reads from an unfocused window.
    fn read(&self) -> Option<String>;
}

/// Writes `value`, then clears it after `after` on a background thread.
pub fn copy_then_clear(
    clipboard: impl Clipboard,
    value: &str,
    after: Duration,
) -> Result<(), String> {
    clipboard.write(value)?;
    let expected = value.to_owned();
    std::thread::spawn(move || {
        std::thread::sleep(after);
        clear_if_unchanged(&clipboard, &expected);
    });
    Ok(())
}

/// Clears the clipboard only if it still holds `expected`. Returns whether it
/// cleared.
///
/// If the user copied something since, wiping would destroy their work. If the
/// clipboard is unreadable, leaving it is the only branch that cannot delete
/// data Vardo did not write. Comparing reads the secret back briefly, a smaller
/// cost than either alternative.
pub fn clear_if_unchanged(clipboard: &impl Clipboard, expected: &str) -> bool {
    match clipboard.read() {
        Some(current) if current == expected => clipboard.write("").is_ok(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct Fake {
        text: Arc<Mutex<Option<String>>>,
    }

    impl Clipboard for Fake {
        fn write(&self, text: &str) -> Result<(), String> {
            *self.text.lock().unwrap() = Some(text.to_owned());
            Ok(())
        }
        fn read(&self) -> Option<String> {
            self.text.lock().unwrap().clone()
        }
    }

    #[test]
    fn clears_what_vardo_wrote() {
        let fake = Fake::default();
        fake.write("hunter2").unwrap();
        assert!(clear_if_unchanged(&fake, "hunter2"));
        assert_eq!(fake.read().as_deref(), Some(""));
    }

    #[test]
    fn keeps_what_the_user_copied_since() {
        let fake = Fake::default();
        fake.write("their notes").unwrap();
        assert!(!clear_if_unchanged(&fake, "hunter2"));
        assert_eq!(fake.read().as_deref(), Some("their notes"));
    }

    #[test]
    fn leaves_an_unreadable_clipboard_alone() {
        let fake = Fake::default();
        assert!(!clear_if_unchanged(&fake, "hunter2"));
        assert_eq!(fake.read(), None);
    }

    #[test]
    fn copy_writes_now_and_clears_later() {
        let fake = Fake::default();
        copy_then_clear(fake.clone(), "hunter2", Duration::from_millis(20)).unwrap();
        assert_eq!(fake.read().as_deref(), Some("hunter2"));
        for _ in 0..100 {
            if fake.read().as_deref() == Some("") {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("the clipboard was never cleared");
    }
}
