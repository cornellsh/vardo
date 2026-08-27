//! Wire types.
//!
//! These mirror `app/src/ipc/types.ts` exactly. They are `camelCase` on the
//! wire because the consumer is TypeScript.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultInfo {
    pub name: String,
    pub path: String,
    pub entry_count: usize,
    pub format: String,
    pub writable: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupNode {
    pub uuid: String,
    pub name: String,
    pub entry_count: usize,
    pub children: Vec<GroupNode>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntrySummary {
    pub uuid: String,
    pub title: String,
    pub username: String,
    pub url_host: String,
    pub modified: i64,
    pub icon_id: u32,
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
#[serde(rename_all = "camelCase")]
pub struct Field {
    pub key: String,
    pub protected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    pub empty: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Times {
    pub created: i64,
    pub modified: i64,
    pub accessed: i64,
    pub expires: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    pub name: String,
    pub size: Option<usize>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryDetail {
    pub uuid: String,
    pub title: String,
    pub fields: Vec<Field>,
    pub tags: Vec<String>,
    pub times: Times,
    pub attachments: Vec<Attachment>,
    pub history_count: usize,
    pub group_path: Vec<String>,
}

#[derive(Debug, Serialize)]
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
#[serde(rename_all = "camelCase")]
pub struct UnlockPhase {
    pub phase: String,
    pub progress: Option<f32>,
}

impl UnlockPhase {
    pub fn new(phase: &str) -> Self {
        Self {
            phase: phase.to_string(),
            progress: None,
        }
    }
}
