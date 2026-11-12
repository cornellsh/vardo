//! `vardo-browser-host`: the program Chrome starts for Vardo's extension.
//!
//! A relay. It reads a message from the extension, hands it to the running
//! Vardo over a local socket, and passes the reply back. It holds no vault and
//! no keys, and links nothing from Vardo except the shared protocol, so the
//! process the browser starts never contains vault code.
//!
//! Run it yourself once to register it with your browsers:
//!
//! ```text
//! vardo-browser-host --install
//! ```
//!
//! Its standard output belongs to Chrome and carries nothing but framed
//! messages. Anything for a person goes to standard error, which Chrome writes
//! to its own log.

#![forbid(unsafe_code)]

use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use vardo_browser::framing;
use vardo_browser::protocol::{ErrorCode, VERSION};

/// The extension's ID, fixed by the public key in its manifest. Chrome only
/// starts this host for the IDs listed when it was installed.
const EXTENSION_ID: &str = "fkemoafedkbgljikdhjbglhlhfplamll";

/// The name the extension asks Chrome for.
const HOST_NAME: &str = "app.vardo.browser";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--install") => exit_with(install()),
        Some("--uninstall") => exit_with(uninstall()),
        Some("--help" | "-h") => {
            eprintln!("vardo-browser-host --install    register with installed browsers");
            eprintln!("vardo-browser-host --uninstall  remove that registration");
        }
        // Chrome passes the calling extension's origin, and on Windows a
        // window handle. Neither needs acting on: Chrome only starts this for
        // the extension IDs in the manifest.
        _ => {
            if let Err(e) = relay() {
                eprintln!("vardo-browser-host: {e}");
                std::process::exit(1);
            }
        }
    }
}

fn exit_with(result: Result<(), String>) {
    if let Err(e) = result {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

// --- relaying ---------------------------------------------------------------

fn relay() -> io::Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut input = stdin.lock();
    let mut output = stdout.lock();
    let mut vardo: Option<Connection> = None;

    while let Some(message) = framing::read(&mut input)? {
        let reply = answer(&message, &mut vardo);
        framing::write(&mut output, reply.as_bytes())?;
    }
    Ok(())
}

/// One reply for one message. Never fails: anything that goes wrong becomes an
/// error message the extension can show, because a host that exits instead
/// leaves the extension with Chrome's generic "native host has exited".
fn answer(message: &[u8], vardo: &mut Option<Connection>) -> String {
    let parsed: Value = match serde_json::from_slice(message) {
        Ok(v) => v,
        Err(_) => return error(0, ErrorCode::BadRequest, "Not a valid message"),
    };
    let id = parsed.get("id").and_then(Value::as_u64).unwrap_or(0);

    if parsed.get("type").and_then(Value::as_str) == Some("launch") {
        return launch(id, vardo);
    }

    // One attempt to reconnect, because Vardo may have restarted since the
    // last message. More than one would just delay the honest answer.
    for _ in 0..2 {
        if vardo.is_none() {
            *vardo = Connection::open();
        }
        let Some(connection) = vardo.as_mut() else {
            return not_running(id);
        };
        match connection.ask(message) {
            Ok(reply) => return reply,
            Err(_) => *vardo = None,
        }
    }
    not_running(id)
}

fn not_running(id: u64) -> String {
    error(id, ErrorCode::NotRunning, "Vardo is not running")
}

fn error(id: u64, code: ErrorCode, message: &str) -> String {
    json!({ "v": VERSION, "id": id, "type": "error", "code": code, "message": message }).to_string()
}

#[cfg(unix)]
struct Connection {
    writer: std::os::unix::net::UnixStream,
    reader: BufReader<std::os::unix::net::UnixStream>,
}

#[cfg(unix)]
impl Connection {
    fn open() -> Option<Self> {
        let stream = std::os::unix::net::UnixStream::connect(vardo_browser::socket::path()).ok()?;
        // Long enough for a save in progress to release the vault, short
        // enough that a hung Vardo does not hang the browser's popup forever.
        let timeout = Some(std::time::Duration::from_secs(10));
        stream.set_read_timeout(timeout).ok()?;
        stream.set_write_timeout(timeout).ok()?;
        let reader = BufReader::new(stream.try_clone().ok()?);
        Some(Connection {
            writer: stream,
            reader,
        })
    }

    /// One JSON object per line. `serde_json` escapes newlines inside strings,
    /// so a line is always exactly one message.
    fn ask(&mut self, message: &[u8]) -> io::Result<String> {
        self.writer.write_all(message)?;
        self.writer.write_all(b"\n")?;
        self.writer.flush()?;
        let mut line = String::new();
        if self.reader.read_line(&mut line)? == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        Ok(line.trim_end().to_string())
    }
}

#[cfg(not(unix))]
struct Connection;

#[cfg(not(unix))]
impl Connection {
    fn open() -> Option<Self> {
        None
    }
    fn ask(&mut self, _message: &[u8]) -> io::Result<String> {
        Err(io::ErrorKind::Unsupported.into())
    }
}

/// Starts Vardo when it is not running.
///
/// Looks beside this program, which is where both are installed. The child is
/// put in its own process group, because Chrome ends the host when the popup
/// closes, and Vardo must not be ended with it.
fn launch(id: u64, vardo: &mut Option<Connection>) -> String {
    if vardo.is_some() || Connection::open().is_some() {
        return json!({ "v": VERSION, "id": id, "type": "launched" }).to_string();
    }
    let Some(app) = app_beside_us() else {
        return error(
            id,
            ErrorCode::NotRunning,
            "Vardo is not installed beside its browser host",
        );
    };
    let mut command = std::process::Command::new(app);
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        command.process_group(0);
    }
    match command.spawn() {
        Ok(_) => json!({ "v": VERSION, "id": id, "type": "launched" }).to_string(),
        Err(e) => error(
            id,
            ErrorCode::Internal,
            &format!("Could not start Vardo: {e}"),
        ),
    }
}

fn app_beside_us() -> Option<PathBuf> {
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    ["vardo", "vardo-app", "vardo.exe", "vardo-app.exe"]
        .iter()
        .map(|name| dir.join(name))
        .find(|p| p.is_file())
}

// --- registering with browsers ----------------------------------------------

/// Each Chromium browser looks for hosts in its own directory, and only these
/// four were checked to work. Others are added once someone confirms them.
fn browser_dirs() -> Vec<(&'static str, PathBuf)> {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return Vec::new();
    };
    let base = if cfg!(target_os = "macos") {
        home.join("Library/Application Support")
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"))
    };
    let mac = cfg!(target_os = "macos");
    vec![
        (
            "Google Chrome",
            base.join(if mac {
                "Google/Chrome"
            } else {
                "google-chrome"
            }),
        ),
        (
            "Chromium",
            base.join(if mac { "Chromium" } else { "chromium" }),
        ),
        ("Brave", base.join("BraveSoftware/Brave-Browser")),
        (
            "Microsoft Edge",
            base.join(if mac {
                "Microsoft Edge"
            } else {
                "microsoft-edge"
            }),
        ),
        (
            "Vivaldi",
            base.join(if mac { "Vivaldi" } else { "vivaldi" }),
        ),
    ]
}

fn install() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("Cannot find this program: {e}"))?;
    let manifest = json!({
        "name": HOST_NAME,
        "description": "Vardo password manager",
        "path": exe,
        "type": "stdio",
        "allowed_origins": [format!("chrome-extension://{EXTENSION_ID}/")],
    });
    let body = serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?;

    let mut installed = 0;
    for (name, dir) in browser_dirs() {
        // Only browsers that have run at least once. Creating a profile
        // directory for a browser that is not installed would be litter.
        if !dir.is_dir() {
            continue;
        }
        let hosts = dir.join("NativeMessagingHosts");
        let file = hosts.join(format!("{HOST_NAME}.json"));
        std::fs::create_dir_all(&hosts)
            .and_then(|()| std::fs::write(&file, &body))
            .map_err(|e| format!("{name}: could not write {}: {e}", file.display()))?;
        eprintln!("Registered with {name}");
        installed += 1;
    }
    if installed == 0 {
        return Err("No supported browser found. Start Chrome, Chromium, Brave, Edge or Vivaldi once, then try again.".into());
    }
    if app_beside_us().is_none() {
        eprintln!(
            "Note: Vardo is not beside this program, so the extension cannot start it for you."
        );
    }
    Ok(())
}

fn uninstall() -> Result<(), String> {
    for (name, dir) in browser_dirs() {
        let file = dir
            .join("NativeMessagingHosts")
            .join(format!("{HOST_NAME}.json"));
        if remove_if_present(&file)? {
            eprintln!("Removed from {name}");
        }
    }
    Ok(())
}

fn remove_if_present(file: &Path) -> Result<bool, String> {
    match std::fs::remove_file(file) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(format!("could not remove {}: {e}", file.display())),
    }
}
