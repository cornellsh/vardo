//! Where Vardo listens for the native host.
//!
//! A Unix socket in the user's private runtime directory. Only that user can
//! reach it. Other programs running as the same user can, and the ADR explains
//! why a secret stored beside it would not change that.
//!
//! Windows is not supported yet; it needs a named pipe, and is listed as later
//! work in ADR-006.

use std::path::PathBuf;

/// The socket's path. The same function runs in Vardo and in the host, so the
/// two cannot disagree about where to meet.
pub fn path() -> PathBuf {
    dir().join("browser.sock")
}

/// The directory holding the socket, created by Vardo with owner-only access.
pub fn dir() -> PathBuf {
    // On Linux this is a per-user tmpfs, already private to the user.
    if let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR").filter(|v| !v.is_empty()) {
        return PathBuf::from(runtime).join("vardo");
    }
    // macOS gives every user their own temporary directory. Elsewhere, the
    // user name keeps two accounts from sharing one directory, and Vardo
    // creates it with owner-only permissions either way.
    let user = std::env::var("USER").unwrap_or_else(|_| "user".into());
    std::env::temp_dir().join(format!("vardo-{user}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_path_fits_in_a_socket_address() {
        // A Unix socket path is limited to 104 bytes on macOS and 108 on Linux,
        // and exceeding it fails with an error that does not mention length.
        assert!(path().as_os_str().len() < 100, "{}", path().display());
    }
}
