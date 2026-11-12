//! The socket the browser extension's native host connects to.
//!
//! Here rather than in the desktop shell so that it can be tested with a real
//! socket and no window. The shell supplies one callback for the two things
//! that need the window toolkit, copying and showing the window, and runs
//! [`serve`] on a thread. Keeps `inv:host-is-a-relay`.
//!
//! Unix only. Windows needs a named pipe (`plan:windows-pipe`).

use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::browser::{handle, Effect};
use crate::Session;

/// Performs an [`Effect`] other than [`Effect::None`]. An error means the
/// effect did not happen, and the extension is told so instead of being told
/// it succeeded.
pub type Perform = dyn Fn(Effect) -> Result<(), String> + Send + Sync;

/// Prepares the socket at `path` and answers connections until the process
/// ends. Returns early only if the socket cannot be set up.
pub fn serve(
    path: &Path,
    session: Arc<Mutex<Option<Session>>>,
    perform: Arc<Perform>,
) -> io::Result<()> {
    let listener = bind(path)?;
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let session = Arc::clone(&session);
        let perform = Arc::clone(&perform);
        std::thread::spawn(move || {
            let _ = connection(stream, &session, perform.as_ref());
        });
    }
    Ok(())
}

/// Creates the directory with owner-only access and claims the socket,
/// refusing if another Vardo already answers on it.
pub fn bind(path: &Path) -> io::Result<UnixListener> {
    if let Some(dir) = path.parent() {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
        // `mode` only applies when the directory is new. One left behind by
        // something else, or created under a looser umask, is tightened here.
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }

    if path.exists() {
        if UnixStream::connect(path).is_ok() {
            // Two would race to answer the same browser, each with its own
            // idea of what is unlocked.
            return Err(io::Error::new(
                io::ErrorKind::AddrInUse,
                "another Vardo is already serving the browser",
            ));
        }
        // Left behind by a Vardo that did not exit cleanly.
        std::fs::remove_file(path)?;
    }

    let listener = UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

/// One native host process, for as long as the browser keeps it.
fn connection(
    stream: UnixStream,
    session: &Mutex<Option<Session>>,
    perform: &Perform,
) -> io::Result<()> {
    let mut writer = stream.try_clone()?;
    for line in BufReader::new(stream).lines() {
        let reply = answer(&line?, session, perform);
        writer.write_all(reply.as_bytes())?;
        writer.write_all(b"\n")?;
        writer.flush()?;
    }
    Ok(())
}

fn answer(line: &str, session: &Mutex<Option<Session>>, perform: &Perform) -> String {
    // The lock is held only to decide. Copying and showing the window happen
    // after it is released, so a slow clipboard cannot block a save.
    let outcome = match session.lock() {
        Ok(guard) => handle(line, guard.as_ref()),
        Err(_) => return internal_error(line, "Vardo hit an internal error. Restart it."),
    };
    let reply = outcome.json();
    match outcome.effect {
        Effect::None => reply,
        effect => match perform(effect) {
            Ok(()) => reply,
            // The prepared reply says it worked, which is now untrue.
            Err(e) => internal_error(line, &e),
        },
    }
}

fn internal_error(line: &str, message: &str) -> String {
    let id = serde_json::from_str::<serde_json::Value>(line)
        .ok()
        .and_then(|v| v.get("id").and_then(serde_json::Value::as_u64))
        .unwrap_or(0);
    serde_json::json!({
        "v": vardo_browser::protocol::VERSION,
        "id": id,
        "type": "error",
        "code": "internal",
        "message": message,
    })
    .to_string()
}
