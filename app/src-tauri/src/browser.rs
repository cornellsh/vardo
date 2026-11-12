//! Connects the browser extension to this window.
//!
//! The socket server and every decision live in `vardo-core`, where they are
//! tested. This supplies the two things that need the window toolkit:
//! copying to the clipboard and showing the window. See ADR-006.

#[cfg(unix)]
pub fn start(app: tauri::AppHandle, state: crate::AppState) {
    use std::sync::Arc;
    use vardo_core::browser::Effect;

    let perform = Arc::new(move |effect: Effect| match effect {
        Effect::None => Ok(()),
        Effect::Show => {
            crate::tray::reveal(&app);
            Ok(())
        }
        Effect::Copy(value) => crate::clipboard::copy_and_schedule_clear(
            &app,
            &value,
            vardo_core::CLIPBOARD_CLEAR_SECONDS,
        )
        .map_err(|e| format!("Could not copy: {e}")),
    });

    std::thread::Builder::new()
        .name("browser-socket".into())
        .spawn(move || {
            let path = vardo_browser::socket::path();
            if let Err(e) = vardo_core::browser_server::serve(&path, state.session, perform) {
                // The rest of Vardo works without the extension, so this is a
                // message, not a reason to stop.
                eprintln!("vardo: browser extension unavailable: {e}");
            }
        })
        .ok();
}

/// Windows needs a named pipe instead. Listed as later work in ADR-006.
#[cfg(not(unix))]
pub fn start(_app: tauri::AppHandle, _state: crate::AppState) {}
