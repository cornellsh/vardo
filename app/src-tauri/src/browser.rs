//! Connects the browser extension to this window.
//!
//! The socket server and every decision live in `vardo-core`, where they are
//! tested. This supplies the things that need the window toolkit: copying to
//! the clipboard, showing the window, and telling the interface the vault
//! changed. See `dec:core-decides-shells-act`.

#[cfg(any(unix, windows))]
pub fn start(app: tauri::AppHandle, state: crate::AppState) {
    use std::sync::Arc;
    use tauri::Emitter;
    use vardo_core::browser::Effect;

    fn changed(app: &tauri::AppHandle) {
        if let Err(e) = app.emit("vault://changed", ()) {
            eprintln!("vardo: could not tell the window the vault changed: {e}");
        }
    }

    let session = state.session.clone();
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
        // The interface refreshes and saves, as after any edit. The login is in
        // the session either way, and locking saves a dirty vault first, so a
        // missed event is logged rather than reported as a failed save.
        Effect::Changed { uuid } => {
            changed(&app);
            // The person is on that site, so fetching its icon tells it nothing
            // new. Off this thread: the socket must not wait on a website.
            let (app, session) = (app.clone(), session.clone());
            std::thread::spawn(move || {
                if let Ok(Some(_)) = vardo_core::session::fetch_icon_shared(&session, &uuid) {
                    changed(&app);
                }
            });
            Ok(())
        }
    });

    std::thread::Builder::new()
        .name("browser-socket".into())
        .spawn(move || {
            #[cfg(unix)]
            let served = vardo_core::browser::server::serve(
                &vardo_browser::socket::path(),
                state.session,
                perform,
            );
            #[cfg(windows)]
            let served = vardo_core::browser::server::serve_pipe(state.session, perform);
            if let Err(e) = served {
                // The rest of Vardo works without the extension, so this is a
                // message, not a reason to stop.
                eprintln!("vardo: browser extension unavailable: {e}");
            }
        })
        .ok();
}

#[cfg(not(any(unix, windows)))]
pub fn start(_app: tauri::AppHandle, _state: crate::AppState) {}
