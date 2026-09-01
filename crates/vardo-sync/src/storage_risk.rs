//! Working out whether a vault path is somewhere dangerous.
//!
//! Pointing a KeePass client at a cloud-synced folder is the single most common
//! way these deployments lose data: the daemon forks the file into
//! `vault (1).kdbx`, or reports a stale mtime for weeks, or holds a lock that
//! makes atomic rename impossible. KeePassXC issues #9140, #8790, #2066,
//! #11969 and #13050 are all this, and #13050 proposes exactly the warning
//! below without having shipped it.
//!
//! # Why not just match the path text
//!
//! Because it is wrong in both directions. A folder called `~/dropbox-backups`
//! that is an ordinary directory gets flagged, and an rclone mount at
//! `~/vaults` does not — even though the second is the one that eats files.
//!
//! So the mount table is consulted first, and the name is only a fallback for
//! platforms where that is unavailable. The name check still earns its place:
//! a Dropbox *daemon* syncing an ordinary local directory leaves no trace in
//! the mount table at all.

use std::path::Path;

/// What kind of storage a vault appears to live on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageRisk {
    /// An ordinary local filesystem. No warning.
    Local,
    /// A userspace filesystem — rclone, GVfs, sshfs, gocryptfs. The dangerous
    /// case: `rename` is frequently not atomic and mtime is often invented.
    Fuse { fs: String },
    /// A network filesystem. Atomic rename is not guaranteed across a network.
    Network { fs: String },
    /// The mount looks ordinary, but the path names a known sync provider, so a
    /// sync daemon is probably watching the directory.
    NameSuggestsSync { provider: String },
}

impl StorageRisk {
    /// Whether this is worth telling the user about at all.
    ///
    /// A FUSE mount is detected but no longer reported. Detection still earns
    /// its keep - it is how `provider_hint` knows a vault sits on Google Drive
    /// and could be opened through the API instead - but a probe on a live
    /// rclone mount showed the save path works exactly as it does on a local
    /// disk, so there is nothing to warn about and warning anyway just teaches
    /// people that this program's warnings do not mean anything.
    pub fn is_risky(&self) -> bool {
        matches!(
            self,
            StorageRisk::Network { .. } | StorageRisk::NameSuggestsSync { .. }
        )
    }

    /// One line, short enough not to wrap in the banner it appears in.
    ///
    /// The long version of this was a four-line paragraph, which in a banner is
    /// a wall of text that gets dismissed unread. The reasoning belongs in
    /// [`Self::why`], behind a disclosure, not in front of someone who only
    /// wanted to open their vault.
    pub fn explain(&self) -> String {
        match self {
            StorageRisk::Local => "Stored on this computer".into(),

            // A FUSE mount is no longer warned about, and this is the note
            // explaining why so it does not get re-added.
            //
            // I had claimed a userspace mount could leave you with two copies
            // of the vault. That is a real thing, and it is a *sync daemon*
            // thing - Dropbox or Drive Desktop watching a local folder and
            // uploading a conflicted "vault (1).kdbx". An rclone mount is not
            // that: it is a passthrough view of the remote, with no second
            // copy and no daemon to race.
            //
            // Probed on a live `fuse.rclone` mount of Google Drive, doing
            // exactly what save() does: write a temp file, fsync it, rename it
            // over the target, fsync the directory. Rename-over-existing was
            // correct, the directory fsync succeeded, nothing was left behind.
            // The warning was describing a hazard that was not there.
            //
            // It was also condescending. Someone who has mounted their cloud
            // storage over FUSE knows what they have done, and being told
            // about it at every unlock is noise from a program that thinks it
            // knows better.
            StorageRisk::Fuse { .. } => String::new(),

            // Kept, because an interrupted write across a network really can
            // leave a partial file, and this one is not a deliberate choice in
            // the way a FUSE mount is.
            StorageRisk::Network { .. } => "This vault is on a network share".into(),

            // The one worth saying. KeePass databases in a synced folder is a
            // documented way to end up with conflicted copies, and it is not
            // obvious - the folder looks like an ordinary directory.
            StorageRisk::NameSuggestsSync { provider } => {
                format!("A {provider} folder can duplicate this file")
            }
        }
    }

    /// True when Vardo can sync this provider itself, and so has a real fix to
    /// offer rather than only a caution.
    pub fn provider_hint(&self) -> Option<&'static str> {
        let text = match self {
            StorageRisk::Fuse { fs } | StorageRisk::Network { fs } => fs.to_ascii_lowercase(),
            StorageRisk::NameSuggestsSync { provider } => provider.to_ascii_lowercase(),
            StorageRisk::Local => return None,
        };
        // rclone mounts are named after the remote often enough to be worth
        // checking, and a Google Drive mount is exactly the case where the
        // advice stops being "move it somewhere else" and becomes one button.
        if text.contains("rclone") || text.contains("drive") || text.contains("google") {
            Some("google-drive")
        } else {
            None
        }
    }

    /// The full reasoning, for whoever opens the disclosure.
    pub fn why(&self) -> &'static str {
        match self {
            StorageRisk::Local => "",
            StorageRisk::Fuse { .. } => "",
            StorageRisk::Network { .. } => {
                "An atomic rename is not guaranteed across a network, so a save interrupted \
                 partway can leave a partial file."
            }
            StorageRisk::NameSuggestsSync { .. } => {
                "A sync daemon rewriting the file underneath Vardo is how KeePass databases \
                 get forked into conflicted copies."
            }
        }
    }

    /// What to do about it. One line.
    pub fn remedy(&self) -> &'static str {
        match self {
            StorageRisk::Local => "",
            // Two different pieces of advice, because for a Drive mount there
            // is an actual fix rather than a suggestion to reorganise: Vardo
            // can talk to Drive itself and merge properly instead of racing a
            // background daemon over the same file.
            _ => "Open it from Google Drive instead",
        }
    }
}

/// Filesystem types that are userspace mounts.
fn is_fuse(fs: &str) -> bool {
    fs.starts_with("fuse") || matches!(fs, "rclone" | "gocryptfs" | "sshfs" | "gvfsd-fuse")
}

/// Filesystem types that live across a network.
fn is_network(fs: &str) -> bool {
    matches!(
        fs,
        "nfs" | "nfs4" | "cifs" | "smb3" | "smbfs" | "afpfs" | "davfs" | "webdav" | "ncpfs"
    )
}

const PROVIDERS: &[(&str, &str)] = &[
    ("google drive", "Google Drive"),
    ("googledrive", "Google Drive"),
    ("/gdrive", "Google Drive"),
    ("dropbox", "Dropbox"),
    ("onedrive", "OneDrive"),
    ("cloudstorage", "iCloud or another provider"),
    ("nextcloud", "Nextcloud"),
    ("owncloud", "ownCloud"),
    ("pcloud", "pCloud"),
    ("yandex.disk", "Yandex.Disk"),
    ("icloud", "iCloud"),
    ("mega", "MEGA"),
];

pub fn assess(path: &Path) -> StorageRisk {
    assess_with(path, &read_mount_table())
}

/// Split out so the mount table can be supplied in tests. Reading `/proc/mounts`
/// in a test would assert something about the machine rather than the code.
pub fn assess_with(path: &Path, mounts: &[(String, String)]) -> StorageRisk {
    if let Some(fs) = filesystem_for(path, mounts) {
        if is_fuse(&fs) {
            return StorageRisk::Fuse { fs: pretty_fs(&fs) };
        }
        if is_network(&fs) {
            return StorageRisk::Network {
                fs: fs.to_uppercase(),
            };
        }
    }

    let text = path.to_string_lossy().to_lowercase();
    for (needle, provider) in PROVIDERS {
        if text.contains(needle) {
            return StorageRisk::NameSuggestsSync {
                provider: (*provider).to_string(),
            };
        }
    }

    StorageRisk::Local
}

/// `fuse.rclone` reads better as `rclone`.
fn pretty_fs(fs: &str) -> String {
    fs.strip_prefix("fuse.").unwrap_or(fs).to_string()
}

/// The filesystem type of the longest mount point that is a prefix of `path`.
///
/// Longest wins because mount points nest: `/` always matches, so a shorter
/// match would report `ext4` for everything.
fn filesystem_for(path: &Path, mounts: &[(String, String)]) -> Option<String> {
    let text = path.to_string_lossy().to_string();
    let mut best: Option<(usize, String)> = None;

    for (point, fs) in mounts {
        let matches = text == *point
            || text.starts_with(&format!("{}/", point.trim_end_matches('/')))
            || point == "/";
        if matches {
            let len = point.len();
            if best.as_ref().is_none_or(|(best_len, _)| len > *best_len) {
                best = Some((len, fs.clone()));
            }
        }
    }

    best.map(|(_, fs)| fs)
}

/// `(mount point, filesystem type)` pairs. Empty on platforms without
/// `/proc/mounts`, which degrades to the name check rather than failing.
fn read_mount_table() -> Vec<(String, String)> {
    let Ok(text) = std::fs::read_to_string("/proc/mounts") else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let _device = fields.next()?;
            let point = fields.next()?;
            let fs = fields.next()?;
            // Mount points are escaped octal in /proc/mounts.
            Some((point.replace("\\040", " "), fs.to_string()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mounts() -> Vec<(String, String)> {
        vec![
            ("/".into(), "ext4".into()),
            ("/home".into(), "ext4".into()),
            ("/home/user/GoogleDrive".into(), "fuse.rclone".into()),
            ("/mnt/nas".into(), "nfs4".into()),
            ("/home/user/Dropbox".into(), "ext4".into()),
        ]
    }

    #[test]
    fn an_rclone_mount_is_detected_by_filesystem_not_by_name() {
        let risk = assess_with(
            Path::new("/home/user/GoogleDrive/passwords.kdbx"),
            &mounts(),
        );
        assert_eq!(
            risk,
            StorageRisk::Fuse {
                fs: "rclone".into()
            }
        );
        // Nothing here asserts on the wording. The message deliberately no
        // longer names the filesystem, because "rclone" means nothing to the
        // person reading it, and a test pinned to copy blocks rewording for
        // no benefit.
    }

    #[test]
    fn an_rclone_mount_at_an_innocent_path_is_still_detected() {
        // Detection, not warning. The case pure name-matching misses entirely,
        // and it still matters: it is how we know a vault living on a Google
        // Drive mount could be opened through the API instead. What it no
        // longer does is interrupt someone about it.
        let mut m = mounts();
        m.push(("/home/user/vaults".into(), "fuse.rclone".into()));
        let risk = assess_with(Path::new("/home/user/vaults/passwords.kdbx"), &m);

        assert!(matches!(risk, StorageRisk::Fuse { .. }), "got {risk:?}");
        assert!(!risk.is_risky(), "a FUSE mount must not be reported");
        assert_eq!(risk.provider_hint(), Some("google-drive"));
    }

    #[test]
    fn a_network_mount_is_flagged_separately() {
        assert_eq!(
            assess_with(Path::new("/mnt/nas/passwords.kdbx"), &mounts()),
            StorageRisk::Network { fs: "NFS4".into() }
        );
    }

    #[test]
    fn a_sync_daemon_on_an_ordinary_directory_is_still_flagged() {
        // Dropbox syncs a normal ext4 directory, so the mount table says
        // nothing. The name check exists for exactly this.
        let risk = assess_with(Path::new("/home/user/Dropbox/passwords.kdbx"), &mounts());
        assert_eq!(
            risk,
            StorageRisk::NameSuggestsSync {
                provider: "Dropbox".into()
            }
        );
    }

    #[test]
    fn ordinary_local_storage_is_not_flagged() {
        let risk = assess_with(Path::new("/home/user/passwords.kdbx"), &mounts());
        assert_eq!(risk, StorageRisk::Local);
        assert!(!risk.is_risky());
    }

    #[test]
    fn the_longest_matching_mount_point_wins() {
        // "/" matches everything, so a naive first-match reports ext4 for the
        // rclone mount and the warning never fires.
        assert_eq!(
            filesystem_for(Path::new("/home/user/GoogleDrive/x.kdbx"), &mounts()).as_deref(),
            Some("fuse.rclone")
        );
    }

    #[test]
    fn a_prefix_that_is_not_a_path_component_does_not_match() {
        // /home/user/GoogleDriveBackup is not inside the /home/user/GoogleDrive
        // mount, so it must resolve to the enclosing /home mount instead of
        // inheriting rclone's risk from a bare string prefix.
        let fs = filesystem_for(Path::new("/home/user/GoogleDriveBackup/x.kdbx"), &mounts());
        assert_eq!(fs.as_deref(), Some("ext4"));

        // It is still flagged, but by name rather than by mount — a different
        // and weaker claim, which is the point of keeping them distinct.
        assert_eq!(
            assess_with(Path::new("/home/user/GoogleDriveBackup/x.kdbx"), &mounts()),
            StorageRisk::NameSuggestsSync {
                provider: "Google Drive".into()
            }
        );
    }

    /// Wide enough for a banner in a narrow window, narrow enough that the
    /// text cannot wrap onto a second line. A warning that wraps is a wall,
    /// and a wall is dismissed unread.
    const ONE_LINE: usize = 48;

    #[test]
    fn every_reported_risk_fits_on_one_line_and_offers_a_fix() {
        // Fuse is absent on purpose. It is still detected, because that is how
        // we know a vault could be opened through Drive's API instead, but a
        // live probe showed the save path behaves exactly as it does on local
        // disk, so it is not reported and has no copy to check.
        for risk in [
            StorageRisk::Network { fs: "NFS4".into() },
            StorageRisk::NameSuggestsSync {
                provider: "Dropbox".into(),
            },
        ] {
            let explain = risk.explain();
            assert!(
                explain.len() <= ONE_LINE,
                "must fit one line ({} chars): {explain:?}",
                explain.len()
            );
            assert!(
                risk.remedy().len() <= ONE_LINE,
                "remedy must fit one line: {:?}",
                risk.remedy()
            );
            assert!(!risk.remedy().is_empty(), "no remedy offered for {risk:?}");

            // The reasoning still has to exist - it just belongs behind a
            // disclosure rather than in front of someone opening their vault.
            assert!(
                risk.why().len() > 60,
                "the explanation must survive somewhere: {risk:?}"
            );
        }
    }
}
