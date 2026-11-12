//! Wire types.
//!
//! Contract: no wire type has a field that can carry a protected value
//! (`inv:secrets-stay-in-rust`).
//!
//! `camelCase` on the wire because the consumer is TypeScript. A test writes
//! them to `app/src/ipc/bindings.ts` (`inv:ipc-types-are-generated`).

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct VaultInfo {
    /// Where the vault lives. The UI needs this to decide what to offer:
    /// a local vault can be moved to Drive, a Drive vault can be disconnected,
    /// and only one of them can be on a suspect mount.
    pub source: crate::source::VaultSource,
    /// A second line under the name, in words. "Google Drive", or the
    /// containing folder.
    pub location: String,
    pub name: String,
    pub path: String,
    pub entry_count: usize,
    pub format: String,
    pub writable: bool,
    /// Present when the vault lives somewhere that can corrupt it. Surfaced at
    /// unlock rather than at save, so it is seen before the first write.
    pub storage_warning: Option<StorageWarning>,
    /// Another Vardo window has this vault open. Edits still work, but if both
    /// save, the later save stops and asks for a merge.
    pub open_elsewhere: bool,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct StorageWarning {
    pub kind: StorageKind,
    pub explanation: String,
    pub remedy: String,
    /// The full reasoning, shown only if the user opens it.
    pub why: String,
    /// Set when Vardo can sync this provider itself, so the banner can offer
    /// the fix instead of only describing the hazard.
    pub provider_hint: Option<String>,
}

/// Why a vault's location can corrupt it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "kebab-case")]
pub enum StorageKind {
    Fuse,
    Network,
    Name,
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct GroupNode {
    pub uuid: String,
    pub name: String,
    pub entry_count: usize,
    pub children: Vec<GroupNode>,
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct EntrySummary {
    pub uuid: String,
    pub title: String,
    pub username: String,
    pub url_host: String,
    pub modified: i64,
    pub icon_id: u32,
    /// A PNG the vault already carries, as a data URL. Costs no network and
    /// leaks nothing, because it is the user's own data.
    pub icon_png: Option<String>,
    pub has_totp: bool,
    pub has_attachments: bool,
}

/// A field on an entry.
///
/// `value` is `None` for every protected field, and `skip_serializing_if` means
/// the key is absent from the JSON rather than present as `null`. That matches
/// the TypeScript declaration, where `value?: string` means "ask Rust", not
/// "empty".
#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct Field {
    pub key: String,
    pub protected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub value: Option<String>,
    pub empty: bool,
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct Times {
    pub created: i64,
    pub modified: i64,
    pub accessed: i64,
    pub expires: Option<i64>,
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    pub name: String,
    pub size: Option<usize>,
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct EntryDetail {
    pub uuid: String,
    /// The vault's own icon for this entry, as a data URL.
    pub icon_png: Option<String>,
    pub title: String,
    pub fields: Vec<Field>,
    pub tags: Vec<String>,
    pub times: Times,
    pub attachments: Vec<Attachment>,
    pub history_count: usize,
    pub group_path: Vec<String>,
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct CopyResult {
    pub clear_after_seconds: u32,
}

/// Progress during unlock.
///
/// `progress` is always `None`: the argon2 crate exposes no per-pass callback,
/// so the core knows which phase it is in but not how far through the
/// derivation. The field exists because the shape is right for a future KDF
/// that can report it, and the UI already renders an indeterminate bar when it
/// is absent.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct UnlockPhase {
    pub phase: crate::session::Phase,
    pub progress: Option<f32>,
}

impl UnlockPhase {
    pub fn new(phase: crate::session::Phase) -> Self {
        Self {
            phase,
            progress: None,
        }
    }
}

/// One field change in an [`crate::Session::update_entry`] call.
///
/// `value: None` means remove the field. That is distinct from `Some("")`,
/// which means the field exists and is empty — KeePass treats those the same
/// for standard fields but not for custom ones.
#[derive(Debug, serde::Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct FieldEdit {
    pub key: String,
    pub value: Option<String>,
    #[serde(default)]
    pub protected: bool,
}

#[derive(Debug, serde::Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct GeneratorOptions {
    pub lowercase: bool,
    pub uppercase: bool,
    pub digits: bool,
    pub symbols: bool,
    pub exclude_ambiguous: bool,
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SaveResult {
    pub bytes_written: usize,
    /// Filename of the rotated backup, if one was made.
    pub backup: Option<String>,
}

/// One line in the merge review.
///
/// The prose lives in Rust rather than the UI so that adding a `Change` variant
/// is a compile error here, not a silently blank row on screen.
#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ChangeView {
    /// Machine-readable, for grouping and icon choice.
    pub kind: String,
    pub title: String,
    /// Human sentence describing exactly what will happen.
    pub detail: String,
    /// "local", "remote", or absent when the change has no side.
    pub side: Option<String>,
}

/// One thing the merge refuses to decide.
#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ConflictView {
    pub kind: String,
    pub title: String,
    pub detail: String,
}

/// How a sync settles what the plan could not decide.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "kebab-case")]
pub enum ConflictPolicy {
    /// Keeps both versions rather than choosing, because a password manager
    /// that silently discards one of two passwords is worse than one that
    /// leaves you two to look at.
    Duplicate,
    KeepLocal,
    KeepRemote,
    Refuse,
}

impl From<ConflictPolicy> for vardo_merge::ConflictPolicy {
    fn from(policy: ConflictPolicy) -> Self {
        match policy {
            ConflictPolicy::Duplicate => Self::Duplicate,
            ConflictPolicy::KeepLocal => Self::KeepLocal,
            ConflictPolicy::KeepRemote => Self::KeepRemote,
            ConflictPolicy::Refuse => Self::Refuse,
        }
    }
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SyncPreview {
    pub remote: String,
    /// Remote is byte-identical to our sync point: nothing to fold in.
    pub remote_unchanged: bool,
    /// True when we have no merge base, in which case nothing can be deleted.
    pub first_sync: bool,
    pub changes: Vec<ChangeView>,
    pub conflicts: Vec<ConflictView>,
    /// Non-fatal things that change what the sync will do.
    pub warnings: Vec<SyncWarning>,
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SyncOutcome {
    pub kind: SyncOutcomeKind,
    pub detail: String,
    /// Non-zero means another writer beat us and the merge was redone.
    pub races_lost: u32,
    pub retries: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "kebab-case")]
pub enum SyncOutcomeKind {
    UpToDate,
    Pushed,
    Merged,
    NeedsReview,
    Created,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "kebab-case")]
pub enum SyncWarningKind {
    NoBase,
    RemoteReadOnly,
    ClockSkew,
}

/// Something the user should know about a sync that does not stop it.
///
/// Distinct from an error: every one of these describes a sync that will still
/// happen, but will do something different from what the user expects unless
/// they are told.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SyncWarning {
    pub kind: SyncWarningKind,
    pub detail: String,
}

/// A one-time code, ready to display.
///
/// Deliberately not the seed. The seed generates every future code and is
/// stored as a protected field for that reason; only this derived value, which
/// expires within seconds, crosses the boundary.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct TotpCode {
    pub code: String,
    pub seconds_remaining: u64,
    pub period: u64,
}

/// Whether Drive can be used at all, and whether it currently is.
///
/// Two flags rather than one, because "this build ships no client id" and "you
/// have not signed in" are different problems and only one of them is the
/// user's to fix.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct DriveStatus {
    pub configured: bool,
    pub connected: bool,
}

/// A vault found in Drive, for the chooser.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct DriveVault {
    pub file_id: String,
    pub name: String,
    pub size: u64,
    pub modified_unix: Option<i64>,
}

/// An earlier version of an entry, as stored by KDBX.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct HistoryVersion {
    /// Position in the stored history, needed to fetch the password later.
    /// Not the display order, which is reversed.
    pub index: usize,
    pub title: String,
    pub username: String,
    pub url: String,
    pub modified: i64,
    /// Whether revealing would show anything.
    pub has_password: bool,
}
