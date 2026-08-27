//! Every `#[tauri::command]` in the app.
//!
//! These live in their own module rather than in `lib.rs` deliberately. A
//! `pub` command in the same module as the `tauri::Builder` collides with the
//! `__cmd__*` / `__tauri_command_name_*` macros the attribute generates, and
//! fails to compile with "defined multiple times / reimported here"
//! (tauri-apps/tauri#3198). Putting them in a submodule and referring to them
//! as `commands::x` in `generate_handler!` is the documented arrangement.

use crate::model::{
    Attachment, CopyResult, EntryDetail, EntrySummary, Field, FieldEdit, GeneratorOptions,
    GroupNode, SaveResult, Times, UnlockPhase, VaultInfo,
};
use crate::{clipboard, AppState, Opened, UiError};

use tauri::Emitter;
use vardo_kdbx::Credentials;
use vardo_vault::{Entry, Group, Vault};

type CmdResult<T> = Result<T, UiError>;

// --- unlock -----------------------------------------------------------------

#[tauri::command]
pub async fn open_vault(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    path: String,
    password: String,
) -> CmdResult<VaultInfo> {
    let emit = |phase: &str| {
        // The KDF exposes no per-pass callback, so the UI is told which phase
        // is running but never a fraction. An indeterminate bar that is honest
        // beats a determinate one that is invented.
        let _ = app.emit("vault://unlock-progress", UnlockPhase::new(phase));
    };

    emit("reading");
    let bytes = std::fs::read(&path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => UiError::NotFound { path: path.clone() },
        _ => UiError::Io {
            detail: e.to_string(),
        },
    })?;

    emit("deriving");
    // Argon2d measures 0.871s on the reference vault and closer to 3s on older
    // hardware. On the async runtime's main thread that would freeze the
    // window, which is precisely the behaviour that makes KeePassXC feel
    // broken, so the derivation goes to a blocking thread.
    // Returns the credentials too: saving has to re-derive the key, and
    // prompting again on every save would be unusable. They stay in a
    // `Zeroizing` buffer inside `Credentials` and die with `lock_vault`.
    let (doc, creds) = tauri::async_runtime::spawn_blocking(move || {
        let creds = Credentials::with_password(&password);
        vardo_kdbx::open(&bytes, &creds).map(|doc| (doc, creds))
    })
    .await
    .map_err(|e| UiError::Io {
        detail: e.to_string(),
    })??;

    emit("decrypting");
    let vault = Vault::open(&doc)?;

    emit("parsing");
    let info = VaultInfo {
        name: vault.database_name(),
        path: path.clone(),
        entry_count: vault.entries().len(),
        format: format!("KDBX {}.{}", doc.version().major, doc.version().minor),
        writable: vault.is_writable(),
    };

    *state.open.lock().unwrap() = Some(Opened {
        vault,
        info: info.clone(),
        creds,
        dirty: false,
    });
    Ok(info)
}

#[tauri::command]
pub fn lock_vault(state: tauri::State<'_, AppState>) {
    if let Some(mut opened) = state.open.lock().unwrap().take() {
        // Explicit rather than relying on drop order, so the intent is legible.
        opened.vault.lock();
    }
}

// --- reading ----------------------------------------------------------------

fn with_vault<T>(
    state: &tauri::State<'_, AppState>,
    f: impl FnOnce(&Vault, &VaultInfo) -> T,
) -> CmdResult<T> {
    let guard = state.open.lock().unwrap();
    let opened = guard.as_ref().ok_or(UiError::Locked)?;
    Ok(f(&opened.vault, &opened.info))
}

#[tauri::command]
pub fn groups(state: tauri::State<'_, AppState>) -> CmdResult<GroupNode> {
    with_vault(&state, |vault, _| {
        vault.root().map(node_of).unwrap_or_else(|| GroupNode {
            uuid: String::new(),
            name: String::new(),
            entry_count: 0,
            children: vec![],
        })
    })
}

fn node_of(g: Group<'_>) -> GroupNode {
    GroupNode {
        uuid: g.uuid().map(|u| u.to_string()).unwrap_or_default(),
        name: g.name(),
        // Direct children only; the rail shows per-group counts, not rollups.
        entry_count: g.entries().count(),
        children: g.groups().map(node_of).collect(),
    }
}

#[tauri::command]
pub fn list_entries(
    state: tauri::State<'_, AppState>,
    group: Option<String>,
    query: Option<String>,
) -> CmdResult<Vec<EntrySummary>> {
    with_vault(&state, |vault, _| {
        let needle = query
            .as_deref()
            .map(str::trim)
            .filter(|q| !q.is_empty())
            .map(str::to_lowercase);

        let mut rows: Vec<EntrySummary> = collect_entries(vault, group.as_deref())
            .into_iter()
            .map(summary_of)
            .filter(|s| match &needle {
                None => true,
                Some(q) => {
                    s.title.to_lowercase().contains(q)
                        || s.username.to_lowercase().contains(q)
                        || s.url_host.to_lowercase().contains(q)
                }
            })
            .collect();

        rows.sort_by(|a, b| {
            a.title
                .to_lowercase()
                .cmp(&b.title.to_lowercase())
                .then(b.modified.cmp(&a.modified))
        });
        rows
    })
}

fn collect_entries<'a>(vault: &'a Vault, group: Option<&str>) -> Vec<Entry<'a>> {
    match group {
        None => vault.entries(),
        Some(want) => {
            let mut out = Vec::new();
            if let Some(root) = vault.root() {
                walk_groups(root, &mut |g| {
                    if g.uuid().map(|u| u.to_string()).as_deref() == Some(want) {
                        out.extend(g.entries());
                    }
                });
            }
            out
        }
    }
}

fn walk_groups<'a>(g: Group<'a>, f: &mut impl FnMut(&Group<'a>)) {
    f(&g);
    for child in g.groups() {
        walk_groups(child, f);
    }
}

fn summary_of(e: Entry<'_>) -> EntrySummary {
    let url = e.url();
    EntrySummary {
        uuid: e.uuid().map(|u| u.to_string()).unwrap_or_default(),
        title: e.title(),
        username: e.username(),
        url_host: host_of(&url),
        modified: millis(e.times().last_modified),
        icon_id: e
            .element()
            .child("IconID")
            .and_then(|c| c.text().parse::<u32>().ok())
            .unwrap_or(0),
        has_totp: e.field("otp").is_some() || e.field("TOTP Seed").is_some(),
        has_attachments: !e.attachments().is_empty(),
    }
}

fn millis(t: Option<vardo_vault::Timestamp>) -> i64 {
    t.map(|t| t.unix_seconds() * 1000).unwrap_or(0)
}

/// Host only, not the full URL: enough to disambiguate two entries with the
/// same title, less to leak into a screenshot.
fn host_of(url: &str) -> String {
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    host.split_once('@')
        .map(|(_, h)| h)
        .unwrap_or(host)
        .to_string()
}

#[tauri::command]
pub fn get_entry(state: tauri::State<'_, AppState>, uuid: String) -> CmdResult<EntryDetail> {
    let guard = state.open.lock().unwrap();
    let opened = guard.as_ref().ok_or(UiError::Locked)?;
    let vault = &opened.vault;

    let entry = find_entry(vault, &uuid).ok_or(UiError::NotFound { path: uuid.clone() })?;
    let times = entry.times();

    Ok(EntryDetail {
        uuid: uuid.clone(),
        title: entry.title(),
        fields: ordered_fields(&entry),
        tags: entry.tags(),
        times: Times {
            created: millis(times.created),
            modified: millis(times.last_modified),
            accessed: millis(times.last_accessed),
            // Only meaningful when the entry actually expires; KDBX stores a
            // date regardless and it is garbage when `expires` is false.
            expires: if times.expires {
                times.expires_at.map(|t| t.unix_seconds() * 1000)
            } else {
                None
            },
        },
        attachments: entry
            .attachments()
            .into_iter()
            .map(|(name, size)| Attachment { name, size })
            .collect(),
        history_count: entry.history().len(),
        group_path: path_to(vault, &uuid),
    })
}

/// KDBX stores `String` elements in whatever order the writing client used, so
/// reading them verbatim produces orders like Password, Title, UserName. Sort
/// into the order every KeePass client displays, with custom fields after the
/// standard ones in their original relative order.
///
/// `Title` is dropped: it is already the pane heading, and repeating it as a
/// field is noise.
fn ordered_fields(entry: &Entry<'_>) -> Vec<Field> {
    const STANDARD: [&str; 4] = ["UserName", "Password", "URL", "Notes"];

    let rank = |key: &str| {
        STANDARD
            .iter()
            .position(|s| *s == key)
            .unwrap_or(STANDARD.len())
    };

    let mut fields: Vec<(usize, Field)> = entry
        .fields()
        .into_iter()
        .filter(|(key, _)| key != "Title")
        .map(|(key, v)| {
            let r = rank(&key);
            (
                r,
                Field {
                    key,
                    // A protected field carries NO value. This is the whole
                    // contract: there is no shape of this struct that can leak
                    // one.
                    value: if v.is_protected() {
                        None
                    } else {
                        Some(v.expose().to_string())
                    },
                    empty: v.expose().is_empty(),
                    protected: v.is_protected(),
                },
            )
        })
        .collect();

    // Stable, so custom fields keep the document's own ordering among themselves.
    fields.sort_by_key(|(r, _)| *r);
    fields.into_iter().map(|(_, f)| f).collect()
}

fn find_entry<'a>(vault: &'a Vault, uuid: &str) -> Option<Entry<'a>> {
    vault
        .entries()
        .into_iter()
        .find(|e| e.uuid().map(|u| u.to_string()).as_deref() == Some(uuid))
}

fn path_to(vault: &Vault, uuid: &str) -> Vec<String> {
    fn descend(g: Group<'_>, uuid: &str, trail: &mut Vec<String>) -> bool {
        trail.push(g.name());
        if g.entries()
            .any(|e| e.uuid().map(|u| u.to_string()).as_deref() == Some(uuid))
        {
            return true;
        }
        for child in g.groups() {
            if descend(child, uuid, trail) {
                return true;
            }
        }
        trail.pop();
        false
    }
    let mut trail = Vec::new();
    if let Some(root) = vault.root() {
        descend(root, uuid, &mut trail);
    }
    trail
}

// --- secrets ----------------------------------------------------------------

/// Seconds before the clipboard is wiped. Matches KeePassXC's default.
const CLIPBOARD_CLEAR_SECONDS: u32 = 15;

#[tauri::command]
pub fn copy_field(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    uuid: String,
    key: String,
) -> CmdResult<CopyResult> {
    let guard = state.open.lock().unwrap();
    let opened = guard.as_ref().ok_or(UiError::Locked)?;
    let entry = find_entry(&opened.vault, &uuid).ok_or(UiError::NotFound { path: uuid.clone() })?;
    let value = entry
        .field(&key)
        .ok_or(UiError::NotFound { path: key.clone() })?;

    clipboard::copy_and_schedule_clear(&app, value.expose(), CLIPBOARD_CLEAR_SECONDS)
        .map_err(|detail| UiError::Io { detail })?;

    Ok(CopyResult {
        clear_after_seconds: CLIPBOARD_CLEAR_SECONDS,
    })
}

/// Return a protected value in plaintext.
///
/// The one place a secret crosses into JavaScript, and only on an explicit user
/// action. Everything else in this module exists to keep this the exception.
#[tauri::command]
pub fn reveal_field(
    state: tauri::State<'_, AppState>,
    uuid: String,
    key: String,
) -> CmdResult<String> {
    let guard = state.open.lock().unwrap();
    let opened = guard.as_ref().ok_or(UiError::Locked)?;
    let entry = find_entry(&opened.vault, &uuid).ok_or(UiError::NotFound { path: uuid.clone() })?;
    let value = entry
        .field(&key)
        .ok_or(UiError::NotFound { path: key.clone() })?;
    Ok(value.expose().to_string())
}

/// Vault paths to offer on the unlock screen.
///
/// Currently just the file named on the command line, so `vardo foo.kdbx`
/// works and file managers can hand us a path. A persisted recents list lands
/// in Phase C; until then an empty result is the honest answer, rather than
/// inventing a path the user never opened.
#[tauri::command]
pub fn recent_vaults() -> Vec<String> {
    std::env::args()
        .skip(1)
        .filter(|a| !a.starts_with('-'))
        .filter(|a| std::path::Path::new(a).is_file())
        .take(1)
        .collect()
}

// --- writing ----------------------------------------------------------------

/// Applies a set of field changes to one entry.
///
/// The whole edit is one call so the vault layer can push exactly one history
/// record: a per-field command would archive a copy per keystroke.
#[tauri::command]
pub fn update_entry(
    state: tauri::State<'_, AppState>,
    uuid: String,
    fields: Vec<FieldEdit>,
) -> CmdResult<bool> {
    let mut guard = state.open.lock().unwrap();
    let opened = guard.as_mut().ok_or(UiError::Locked)?;
    let id = parse_uuid(&uuid)?;

    let changed = opened.vault.update_entry(id, |e| {
        for f in &fields {
            match &f.value {
                Some(value) => e.set_field(&f.key, value, f.protected),
                None => e.remove_field(&f.key),
            }
        }
    })?;

    opened.dirty |= changed;
    Ok(changed)
}

#[tauri::command]
pub fn create_entry(state: tauri::State<'_, AppState>, group: String) -> CmdResult<String> {
    let mut guard = state.open.lock().unwrap();
    let opened = guard.as_mut().ok_or(UiError::Locked)?;
    let group = parse_uuid(&group)?;
    let uuid = opened.vault.create_entry(group)?;
    opened.dirty = true;
    Ok(uuid.to_base64())
}

#[tauri::command]
pub fn delete_entry(state: tauri::State<'_, AppState>, uuid: String) -> CmdResult<String> {
    let mut guard = state.open.lock().unwrap();
    let opened = guard.as_mut().ok_or(UiError::Locked)?;
    let outcome = opened.vault.delete_entry(parse_uuid(&uuid)?)?;
    opened.dirty = true;
    Ok(match outcome {
        vardo_vault::Deletion::Recycled => "recycled".into(),
        vardo_vault::Deletion::Purged => "purged".into(),
    })
}

#[tauri::command]
pub fn move_entry(state: tauri::State<'_, AppState>, uuid: String, group: String) -> CmdResult<()> {
    let mut guard = state.open.lock().unwrap();
    let opened = guard.as_mut().ok_or(UiError::Locked)?;
    opened
        .vault
        .move_entry(parse_uuid(&uuid)?, parse_uuid(&group)?)?;
    opened.dirty = true;
    Ok(())
}

/// Writes the vault to disk, verifying the encoded bytes re-open before
/// anything is replaced. See `save.rs`.
#[tauri::command]
pub fn save_vault(state: tauri::State<'_, AppState>) -> CmdResult<SaveResult> {
    let mut guard = state.open.lock().unwrap();
    let opened = guard.as_mut().ok_or(UiError::Locked)?;

    let saved = crate::save::save(
        &opened.vault,
        &opened.creds,
        std::path::Path::new(&opened.info.path),
    )?;

    opened.dirty = false;
    Ok(SaveResult {
        bytes_written: saved.bytes_written,
        backup: saved.backup.map(|p| {
            p.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        }),
    })
}

#[tauri::command]
pub fn is_dirty(state: tauri::State<'_, AppState>) -> CmdResult<bool> {
    let guard = state.open.lock().unwrap();
    Ok(guard.as_ref().map(|o| o.dirty).unwrap_or(false))
}

/// Generates a password.
///
/// Lives in Rust so the generated value can go straight into the vault without
/// a detour through a JS string, and so it uses the OS CSPRNG rather than
/// `Math.random`, which is not cryptographically secure and has been the source
/// of real weak-password bugs in browser-based generators.
#[tauri::command]
pub fn generate_password(length: usize, options: GeneratorOptions) -> CmdResult<String> {
    crate::generate::password(length, &options).map_err(|detail| UiError::Io { detail })
}

fn parse_uuid(text: &str) -> CmdResult<vardo_vault::Uuid> {
    vardo_vault::Uuid::parse(text).ok_or(UiError::NotFound {
        path: text.to_string(),
    })
}
