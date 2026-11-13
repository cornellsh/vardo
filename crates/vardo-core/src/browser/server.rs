//! The socket the browser extension's native host connects to.
//!
//! Contract: the socket sits in a directory only this user can enter, and the
//! host only relays bytes (`inv:host-is-a-relay`).
//!
//! Here rather than in the desktop shell so that it can be tested with a real
//! socket and no window. The shell supplies one callback for the two things
//! that need the window toolkit, copying and showing the window, and runs
//! [`serve`] on a thread. Keeps `inv:host-is-a-relay`.
//!
//! On Windows it is a named pipe with a random name, written to a file only
//! this user can read ([`serve_pipe`]). Anyone could create a pipe with a
//! fixed name before Vardo starts and receive what the extension sends.

use std::io::{self, BufRead, BufReader, Write};
#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
#[cfg(unix)]
use std::os::unix::net::{UnixListener, UnixStream};
#[cfg(unix)]
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
#[cfg(unix)]
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
            let Ok(reader) = stream.try_clone() else {
                return;
            };
            let _ = connection(BufReader::new(reader), stream, &session, perform.as_ref());
        });
    }
    Ok(())
}

/// Creates the directory with owner-only access and claims the socket,
/// refusing if another Vardo already answers on it.
#[cfg(unix)]
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
    reader: impl BufRead,
    mut writer: impl Write,
    session: &Mutex<Option<Session>>,
    perform: &Perform,
) -> io::Result<()> {
    for line in reader.lines() {
        let reply = answer(&line?, session, perform);
        writer.write_all(reply.as_bytes())?;
        writer.write_all(b"\n")?;
        writer.flush()?;
    }
    Ok(())
}

/// Listens on a named pipe, with a name nobody else can guess.
///
/// A fresh random name each start, published in `pipe_name_file` under the
/// user's own profile. Refuses when another Vardo is already listening on the
/// published pipe, for the same reason `bind` does.
#[cfg(windows)]
pub fn serve_pipe(session: Arc<Mutex<Option<Session>>>, perform: Arc<Perform>) -> io::Result<()> {
    use interprocess::os::windows::named_pipe::{pipe_mode, PipeListenerOptions};

    let file = vardo_browser::socket::pipe_name_file()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "LOCALAPPDATA is not set"))?;
    if let Some(live) = vardo_browser::socket::pipe_name() {
        if std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&live)
            .is_ok()
        {
            return Err(io::Error::new(
                io::ErrorKind::AddrInUse,
                "another Vardo is already serving the browser",
            ));
        }
    }

    let mut random = [0u8; 16];
    getrandom::fill(&mut random).map_err(|e| io::Error::other(e.to_string()))?;
    let hex: String = random.iter().map(|b| format!("{b:02x}")).collect();
    let name = format!(r"\\.\pipe\vardo-{hex}");

    // Created before the name is published, so a host never finds a name with
    // nothing behind it. Remote clients are refused by default.
    let listener = PipeListenerOptions::new()
        .path(name.as_str())
        .create_duplex::<pipe_mode::Bytes>()?;
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&file, &name)?;

    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let session = Arc::clone(&session);
        let perform = Arc::clone(&perform);
        std::thread::spawn(move || {
            let (recv, send) = stream.split();
            let _ = connection(BufReader::new(recv), send, &session, perform.as_ref());
        });
    }
    Ok(())
}

fn answer(line: &str, session: &Mutex<Option<Session>>, perform: &Perform) -> String {
    // The lock is held only to decide. Copying and showing the window happen
    // after it is released, so a slow clipboard cannot block a save.
    let outcome = match session.lock() {
        Ok(mut guard) => handle(line, guard.as_mut()),
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
