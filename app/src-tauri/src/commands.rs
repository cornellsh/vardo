//! Every `#[tauri::command]` in the app.
//!
//! Deliberately thin. Each function deserialises, takes the session lock, calls
//! [`vardo_core`], and serialises back — nothing here makes a decision. The
//! logic lives in `vardo-core` so it can be tested without a window; see that
//! crate's module docs for why.
//!
//! These live in their own module rather than in `lib.rs` because a `pub`
//! command in the same module as the `tauri::Builder` collides with the
//! `__cmd__*` macros the attribute generates (tauri-apps/tauri#3198).

use tauri::{Emitter, Manager};
use vardo_core::model::{
    EntryDetail, EntrySummary, FieldEdit, GeneratorOptions, GroupNode, SaveResult, SyncOutcome,
    SyncPreview, UnlockPhase, VaultInfo,
};
use vardo_core::{CoreError, Session};

use crate::AppState;

type CmdResult<T> = Result<T, CoreError>;

/// Runs `f` against the unlocked session, or reports that it is locked.
fn with_session<T>(
    state: &tauri::State<'_, AppState>,
    f: impl FnOnce(&Session) -> Result<T, CoreError>,
) -> CmdResult<T> {
    let guard = state.session.lock().unwrap();
    let session = guard.as_ref().ok_or(CoreError::Locked)?;
    f(session)
}

fn with_session_mut<T>(
    state: &tauri::State<'_, AppState>,
    f: impl FnOnce(&mut Session) -> Result<T, CoreError>,
) -> CmdResult<T> {
    let mut guard = state.session.lock().unwrap();
    let session = guard.as_mut().ok_or(CoreError::Locked)?;
    f(session)
}

// --- unlock -----------------------------------------------------------------

#[tauri::command]
pub async fn open_vault(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    path: String,
    password: String,
) -> CmdResult<VaultInfo> {
    let handle = app.clone();
    // Argon2d measures 0.871s on the reference vault and closer to 3s on older
    // hardware. On the async runtime's main thread that would freeze the
    // window, which is precisely the behaviour that makes KeePassXC feel
    // broken, so the whole unlock goes to a blocking thread.
    let session = tauri::async_runtime::spawn_blocking(move || {
        Session::open(std::path::Path::new(&path), &password, |phase| {
            let _ = handle.emit("vault://unlock-progress", UnlockPhase::new(phase.as_str()));
        })
    })
    .await
    .map_err(|e| CoreError::Io {
        detail: e.to_string(),
    })??;

    let info = session.info().clone();
    *state.session.lock().unwrap() = Some(session);
    Ok(info)
}

#[tauri::command]
pub fn lock_vault(state: tauri::State<'_, AppState>) {
    if let Some(mut session) = state.session.lock().unwrap().take() {
        session.lock();
    }
}

// --- reading ----------------------------------------------------------------

#[tauri::command]
pub fn vault_info(state: tauri::State<'_, AppState>) -> CmdResult<VaultInfo> {
    with_session(&state, |s| Ok(s.info().clone()))
}

#[tauri::command]
pub fn groups(state: tauri::State<'_, AppState>) -> CmdResult<GroupNode> {
    with_session(&state, |s| Ok(s.groups()))
}

#[tauri::command]
pub fn list_entries(
    state: tauri::State<'_, AppState>,
    group: Option<String>,
    query: Option<String>,
) -> CmdResult<Vec<EntrySummary>> {
    with_session(&state, |s| {
        Ok(s.list_entries(group.as_deref(), query.as_deref()))
    })
}

#[tauri::command]
pub fn get_entry(state: tauri::State<'_, AppState>, uuid: String) -> CmdResult<EntryDetail> {
    with_session(&state, |s| s.entry(&uuid))
}

// --- secrets ----------------------------------------------------------------

#[tauri::command]
pub fn copy_field(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    uuid: String,
    key: String,
) -> CmdResult<vardo_core::model::CopyResult> {
    let value = with_session(&state, |s| s.field_value(&uuid, &key))?;
    crate::clipboard::copy_and_schedule_clear(&app, &value, vardo_core::CLIPBOARD_CLEAR_SECONDS)
        .map_err(|detail| CoreError::Io { detail })?;
    Ok(vardo_core::model::CopyResult {
        clear_after_seconds: vardo_core::CLIPBOARD_CLEAR_SECONDS,
    })
}

/// Return a protected value in plaintext.
///
/// The one place a secret crosses into JavaScript, and only on an explicit user
/// action. Everything else exists to keep this the exception.
#[tauri::command]
pub fn reveal_field(
    state: tauri::State<'_, AppState>,
    uuid: String,
    key: String,
) -> CmdResult<String> {
    with_session(&state, |s| Ok(s.field_value(&uuid, &key)?.to_string()))
}

// --- writing ----------------------------------------------------------------

#[tauri::command]
pub fn update_entry(
    state: tauri::State<'_, AppState>,
    uuid: String,
    fields: Vec<FieldEdit>,
) -> CmdResult<bool> {
    with_session_mut(&state, |s| s.update_entry(&uuid, &fields))
}

#[tauri::command]
pub fn create_entry(state: tauri::State<'_, AppState>, group: String) -> CmdResult<String> {
    with_session_mut(&state, |s| s.create_entry(&group))
}

#[tauri::command]
pub fn delete_entry(state: tauri::State<'_, AppState>, uuid: String) -> CmdResult<String> {
    with_session_mut(&state, |s| Ok(s.delete_entry(&uuid)?.to_string()))
}

#[tauri::command]
pub fn move_entry(state: tauri::State<'_, AppState>, uuid: String, group: String) -> CmdResult<()> {
    with_session_mut(&state, |s| s.move_entry(&uuid, &group))
}

#[tauri::command]
pub fn is_dirty(state: tauri::State<'_, AppState>) -> CmdResult<bool> {
    let guard = state.session.lock().unwrap();
    Ok(guard.as_ref().is_some_and(|s| s.is_dirty()))
}

#[tauri::command]
pub fn generate_password(length: usize, options: GeneratorOptions) -> CmdResult<String> {
    vardo_core::generate::password(length, &options).map_err(|detail| CoreError::Io { detail })
}

/// Vault paths to offer on the unlock screen.
///
/// Currently just the file named on the command line, so `vardo foo.kdbx`
/// works and file managers can hand us a path. A persisted recents list lands
/// later; until then an empty result is the honest answer.
#[tauri::command]
pub fn recent_vaults() -> Vec<String> {
    std::env::args()
        .skip(1)
        .filter(|a| !a.starts_with('-'))
        .filter(|a| std::path::Path::new(a).is_file())
        .take(1)
        .collect()
}

// --- sync -------------------------------------------------------------------

#[tauri::command]
pub fn sync_preview(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> CmdResult<SyncPreview> {
    let dir = app_dir(&app)?;
    with_session(&state, |s| s.sync_preview(&dir))
}

#[tauri::command]
pub fn sync_apply(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    policy: String,
) -> CmdResult<SyncOutcome> {
    let dir = app_dir(&app)?;
    with_session_mut(&state, |s| s.sync_apply(&dir, &policy))
}

// --- saving -----------------------------------------------------------------

/// Writes the vault to disk.
///
/// Blocking, because saving does two full KDF passes — one to encrypt and one
/// to verify the result re-opens before anything is replaced. That is roughly
/// 1.7s on the reference vault, and doing it on the async runtime's thread
/// would freeze the window exactly as a synchronous unlock would.
#[tauri::command]
pub async fn save_vault(state: tauri::State<'_, AppState>) -> CmdResult<SaveResult> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut guard = state.session.lock().unwrap();
        let session = guard.as_mut().ok_or(CoreError::Locked)?;
        session.save()
    })
    .await
    .map_err(|e| CoreError::Io {
        detail: e.to_string(),
    })?
}

fn app_dir(app: &tauri::AppHandle) -> CmdResult<std::path::PathBuf> {
    app.path().app_config_dir().map_err(|e| CoreError::Io {
        detail: format!("no config directory: {e}"),
    })
}

/// The current one-time code for an entry, or `None` if it has no seed.
#[tauri::command]
pub fn totp(
    state: tauri::State<'_, AppState>,
    uuid: String,
) -> CmdResult<Option<vardo_core::model::TotpCode>> {
    with_session(&state, |s| s.totp(&uuid))
}
