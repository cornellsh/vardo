//! Errors as the interface sees them.
//!
//! Contract: every fallible core call returns [`CoreError`]. It tells a wrong
//! password from a corrupt file no more finely than KDBX itself does. It
//! serialises to the `CoreError` union in `app/src/ipc/bindings.ts`
//! (`inv:ipc-types-are-generated`).

use serde::Serialize;

/// Errors as the UI models them.
///
/// Deliberately coarse. A caller must not be able to distinguish a wrong
/// password from a corrupt file any more finely than KDBX itself does.
#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum CoreError {
    WrongPassword,
    NotFound {
        path: String,
    },
    Corrupt {
        detail: String,
    },
    Unsupported {
        detail: String,
    },
    Io {
        detail: String,
    },
    Locked,
    /// The file we sync against is gone. Distinct from [`CoreError::NotFound`]
    /// so the UI can offer to re-create or re-point rather than saying the
    /// vault is missing, which it is not.
    RemoteMissing {
        path: String,
    },
    /// The other copy opens with a different password than this one. Reporting
    /// it as a wrong password would send the user to re-check the one that is
    /// working.
    RemotePassword,
    /// Ran out of space. Called out separately because the answer is different
    /// from every other IO failure, and because the vault on disk is intact.
    OutOfSpace,
    /// This vault syncs to Drive but no account is connected.
    DriveNotConnected,
    /// The Drive grant was revoked or expired. Not a transport failure, so
    /// "check your connection" would send the user somewhere useless.
    DriveAuthExpired {
        detail: String,
    },
    /// Could not reach the remote at all. Kept apart from every other failure
    /// because it is the only one where the right advice is "try again later"
    /// and where nothing is wrong with the vault, the password, or the grant.
    Offline {
        detail: String,
    },
    /// The stored copy changed since we loaded it, so saving would destroy
    /// whatever changed it.
    ///
    /// Not a failure in any technical sense - the write would succeed. It is
    /// refused because succeeding is the harm: someone opens the vault on a
    /// laptop, adds a password on their phone, then saves on the laptop.
    /// Nothing in that sequence looks dangerous to the person doing it.
    RemoteChanged {
        detail: String,
    },
}

impl std::fmt::Display for CoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            // Every arm here is one short line. These strings land in banners
            // and side panels, and anything that wraps to a paragraph there is
            // a wall of text nobody reads. Longer causes go in a trailing
            // parenthetical, which the UI folds behind a disclosure.
            CoreError::WrongPassword => write!(f, "Wrong password"),
            CoreError::NotFound { path } => write!(f, "Not found: {path}"),
            CoreError::Corrupt { detail }
            | CoreError::Unsupported { detail }
            | CoreError::Io { detail } => write!(f, "{detail}"),
            CoreError::Locked => write!(f, "The vault is locked"),
            // The path is deliberately dropped: whatever screen shows this
            // already names the vault, and interpolating a long path is what
            // pushed the sentence onto a second line.
            CoreError::RemoteMissing { .. } => write!(f, "The other copy is gone"),
            CoreError::RemotePassword => write!(f, "The other copy has a different password"),
            CoreError::OutOfSpace => write!(f, "Not enough space to save"),
            CoreError::DriveNotConnected => write!(f, "No Google account connected"),
            // Shows `detail` rather than a fixed sentence. The generic version
            // discarded the specific explanation the caller had already
            // written - including which file it was about and, for a replaced
            // file, why a vault plainly still sitting in Drive is unreachable.
            CoreError::DriveAuthExpired { detail } => write!(f, "{detail}"),
            CoreError::Offline { detail } => write!(f, "{detail}"),
            CoreError::RemoteChanged { .. } => write!(f, "This vault changed elsewhere"),
        }
    }
}

impl std::error::Error for CoreError {}

impl From<vardo_kdbx::Error> for CoreError {
    fn from(e: vardo_kdbx::Error) -> Self {
        use vardo_kdbx::Error as E;
        match e {
            // A wrong password and a tampered file are the same event here, and
            // that is a property of KDBX rather than a shortcut: the header MAC
            // is computed from the derived key, so a bad key and corrupt bytes
            // fail identically. Reporting them separately would invent a signal
            // the format deliberately withholds.
            E::HeaderAuthFailed | E::BlockAuthFailed { .. } => CoreError::WrongPassword,
            E::UnsupportedVersion { major, minor } => CoreError::Unsupported {
                detail: format!("KDBX {major}.{minor}"),
            },
            E::UnknownCipher(d) => CoreError::Unsupported {
                detail: format!("cipher {d}"),
            },
            E::UnknownKdf(d) => CoreError::Unsupported {
                detail: format!("KDF {d}"),
            },
            E::ResourceLimit(d) => CoreError::Unsupported { detail: d },
            other => CoreError::Corrupt {
                detail: other.to_string(),
            },
        }
    }
}

impl From<vardo_vault::EditError> for CoreError {
    fn from(e: vardo_vault::EditError) -> Self {
        use vardo_vault::EditError as E;
        match e {
            E::ReadOnly => CoreError::Unsupported {
                detail: "KDBX 3.1 databases are read-only in Vardo".into(),
            },
            E::NoSuchEntry(u) | E::NoSuchGroup(u) => CoreError::NotFound {
                path: u.to_base64(),
            },
            other => CoreError::Corrupt {
                detail: other.to_string(),
            },
        }
    }
}
