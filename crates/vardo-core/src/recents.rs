//! The list of vaults this user has opened.
//!
//! Contract: the list holds sources, never secrets. It is written 0600, a
//! corrupt or missing file reads as empty, and it keeps at most `MAX` entries.
//!
//! Deliberately holds no secrets: a source is a path or a Drive file id, and
//! neither is a credential. It is still worth being careful with, because a
//! list of vault filenames is a small map of someone's life, so the file is
//! written 0600 like everything else here.
//!
//! Failures are swallowed on read and reported on write. Losing the recents
//! list is an inconvenience; refusing to start because it could not be parsed
//! would be locking someone out of their passwords over a convenience feature.

use std::path::{Path, PathBuf};

use crate::source::VaultSource;
use crate::CoreError;

/// Enough to be useful, few enough that the chooser stays a glance rather than
/// a search. Beyond this the honest answer is a file picker.
const MAX: usize = 10;

fn store_path(app_dir: &Path) -> PathBuf {
    app_dir.join("recents.json")
}

/// Most recent first. Never fails: a missing or unreadable list is empty.
pub fn load(app_dir: &Path) -> Vec<VaultSource> {
    let Ok(text) = std::fs::read_to_string(store_path(app_dir)) else {
        return Vec::new();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

/// Moves `source` to the front, adding it if new.
///
/// Moving rather than appending is what makes the list useful: the vault you
/// just opened is overwhelmingly the one you will open next, and a list ordered
/// by first-ever-use would put it last.
pub fn remember(app_dir: &Path, source: &VaultSource) -> Result<(), CoreError> {
    let mut list = load(app_dir);
    list.retain(|s| s != source);
    list.insert(0, source.clone());
    list.truncate(MAX);
    write(app_dir, &list)
}

/// The chooser's list: a vault named on the command line first, then the
/// remembered ones without it.
///
/// The argument keeps `vardo foo.kdbx` and file-manager association working.
/// Flags and paths that are not files are skipped. `app_dir` is `None` when
/// the shell could not resolve it, which leaves only the argument.
pub fn chooser_list(
    app_dir: Option<&Path>,
    args: impl IntoIterator<Item = String>,
) -> Vec<VaultSource> {
    let mut out: Vec<VaultSource> = args
        .into_iter()
        .filter(|a| !a.starts_with('-') && Path::new(a).is_file())
        .take(1)
        .map(VaultSource::file)
        .collect();
    for source in app_dir.map(load).unwrap_or_default() {
        if !out.contains(&source) {
            out.push(source);
        }
    }
    out
}

pub fn forget(app_dir: &Path, source: &VaultSource) -> Result<(), CoreError> {
    let mut list = load(app_dir);
    list.retain(|s| s != source);
    write(app_dir, &list)
}

fn write(app_dir: &Path, list: &[VaultSource]) -> Result<(), CoreError> {
    std::fs::create_dir_all(app_dir).map_err(io)?;
    let path = store_path(app_dir);
    let json = serde_json::to_string_pretty(list).map_err(|e| CoreError::Io {
        detail: e.to_string(),
    })?;

    // Written through a temporary and renamed, so an interrupted write leaves
    // the previous list rather than a truncated one that parses to empty.
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(io)?;
    restrict(&tmp);
    std::fs::rename(&tmp, &path).map_err(io)
}

#[cfg(unix)]
fn restrict(path: &Path) {
    use std::os::unix::fs::PermissionsExt as _;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict(_path: &Path) {}

fn io(e: std::io::Error) -> CoreError {
    CoreError::Io {
        detail: e.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vardo-recents-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_missing_list_is_empty_rather_than_an_error() {
        assert!(load(Path::new("/nonexistent/vardo-nope")).is_empty());
    }

    #[test]
    fn a_corrupt_list_is_empty_rather_than_fatal() {
        // The alternative is refusing to show the unlock screen because a
        // convenience file got mangled, which would strand someone from their
        // passwords over something that does not matter.
        let dir = tmp("corrupt");
        std::fs::write(store_path(&dir), "{ this is not json").unwrap();
        assert!(load(&dir).is_empty());
    }

    #[test]
    fn reopening_moves_a_vault_to_the_front() {
        let dir = tmp("order");
        let a = VaultSource::file("/a.kdbx");
        let b = VaultSource::file("/b.kdbx");

        remember(&dir, &a).unwrap();
        remember(&dir, &b).unwrap();
        assert_eq!(load(&dir), vec![b.clone(), a.clone()]);

        remember(&dir, &a).unwrap();
        assert_eq!(load(&dir), vec![a, b], "the one just opened must lead");
    }

    #[test]
    fn a_vault_is_never_listed_twice() {
        let dir = tmp("dedupe");
        let a = VaultSource::file("/a.kdbx");
        for _ in 0..5 {
            remember(&dir, &a).unwrap();
        }
        assert_eq!(load(&dir).len(), 1);
    }

    #[test]
    fn the_list_is_capped() {
        let dir = tmp("cap");
        for i in 0..(MAX + 5) {
            remember(&dir, &VaultSource::file(format!("/v{i}.kdbx"))).unwrap();
        }
        assert_eq!(load(&dir).len(), MAX);
    }

    #[test]
    fn forgetting_removes_only_the_named_vault() {
        let dir = tmp("forget");
        let a = VaultSource::file("/a.kdbx");
        let b = VaultSource::file("/b.kdbx");
        remember(&dir, &a).unwrap();
        remember(&dir, &b).unwrap();

        forget(&dir, &a).unwrap();
        assert_eq!(load(&dir), vec![b]);
    }

    #[test]
    fn a_local_and_a_drive_vault_with_the_same_name_are_separate_entries() {
        // They look identical in the list, so if they collided here the user
        // would silently lose one of them from their recents.
        let dir = tmp("samename");
        let local = VaultSource::file("/home/me/passwords.kdbx");
        let drive = VaultSource::GoogleDrive {
            file_id: "1AbC".into(),
            name: "passwords.kdbx".into(),
        };
        remember(&dir, &local).unwrap();
        remember(&dir, &drive).unwrap();
        assert_eq!(load(&dir).len(), 2);
    }

    #[test]
    fn a_vault_named_on_the_command_line_leads_and_is_not_repeated() {
        let dir = tmp("args");
        let named = dir.join("named.kdbx");
        std::fs::write(&named, b"").unwrap();
        let named = named.to_string_lossy().into_owned();
        let other = VaultSource::file("/other.kdbx");
        remember(&dir, &other).unwrap();
        remember(&dir, &VaultSource::file(&named)).unwrap();

        let list = chooser_list(Some(&dir), ["--flag".into(), named.clone()]);
        assert_eq!(list, vec![VaultSource::file(&named), other]);
    }

    #[test]
    fn a_command_line_path_that_is_not_a_file_is_ignored() {
        let list = chooser_list(None, ["/nonexistent/vardo.kdbx".to_string()]);
        assert!(list.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn the_list_is_not_world_readable() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tmp("perms");
        remember(&dir, &VaultSource::file("/a.kdbx")).unwrap();
        let mode = std::fs::metadata(store_path(&dir))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o077, 0, "recents must not be readable by others");
    }
}
