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
use vardo_core::drive::auth::DriveClient;
use vardo_core::model::{
    DriveVault, EntryDetail, EntrySummary, FieldEdit, GeneratorOptions, GroupNode, SaveResult,
    SyncOutcome, SyncPreview, UnlockPhase, VaultInfo,
};
use vardo_core::source::VaultSource;
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
    source: VaultSource,
    password: String,
) -> CmdResult<VaultInfo> {
    let handle = app.clone();
    let ctx = source_context(&app)?;
    let remembered = source.clone();
    // Argon2d measures 0.871s on the reference vault and closer to 3s on older
    // hardware. On the async runtime's main thread that would freeze the
    // window, which is precisely the behaviour that makes KeePassXC feel
    // broken, so the whole unlock goes to a blocking thread.
    let session = tauri::async_runtime::spawn_blocking(move || {
        Session::open(&source, &ctx, &password, |phase| {
            let _ = handle.emit("vault://unlock-progress", UnlockPhase::new(phase));
        })
    })
    .await
    .map_err(|e| CoreError::Io {
        detail: e.to_string(),
    })??;

    // Recorded only after it opens, so a mistyped path or a vault that no
    // longer exists never lands in the list the user is offered next time.
    if let Ok(dir) = app_dir(&app) {
        let _ = vardo_core::recents::remember(&dir, &remembered);
    }

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
pub fn list_entries_sorted(
    state: tauri::State<'_, AppState>,
    group: Option<String>,
    query: Option<String>,
    sort: String,
) -> CmdResult<Vec<EntrySummary>> {
    with_session(&state, |s| {
        Ok(s.list_entries_sorted(group.as_deref(), query.as_deref(), &sort))
    })
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
/// Vaults this user has opened before, most recent first.
///
/// A file named on the command line is prepended, so `vardo foo.kdbx` and
/// file-manager association keep working.
#[tauri::command]
pub fn recent_vaults(app: tauri::AppHandle) -> Vec<VaultSource> {
    let dir = app_dir(&app).ok();
    vardo_core::recents::chooser_list(dir.as_deref(), std::env::args().skip(1))
}

/// Removes a vault from the list without touching the vault itself.
///
/// Needed because a recents entry can outlive the thing it points at - an
/// unplugged drive, a file deleted outside Vardo - and an entry that can only
/// ever fail must be removable.
#[tauri::command]
pub fn forget_vault(app: tauri::AppHandle, source: VaultSource) -> CmdResult<()> {
    let dir = app_dir(&app)?;
    // Forgetting a WebDAV vault also forgets its server password.
    if let VaultSource::WebDav { url, username } = &source {
        vardo_core::webdav::forget(&dir, url, username);
    }
    vardo_core::recents::forget(&dir, &source)
}

#[tauri::command]
pub fn restore_version(
    state: tauri::State<'_, AppState>,
    uuid: String,
    index: usize,
) -> CmdResult<()> {
    with_session_mut(&state, |s| s.restore_version(&uuid, index))
}

/// Checks a WebDAV vault's address and credentials, and remembers both.
#[tauri::command]
pub async fn webdav_connect(
    app: tauri::AppHandle,
    url: String,
    username: String,
    password: String,
) -> CmdResult<VaultSource> {
    let dir = app_dir(&app)?;
    // Blocking: one HTTP round trip, which must not stall the async runtime.
    let source = tauri::async_runtime::spawn_blocking({
        let dir = dir.clone();
        move || vardo_core::webdav::connect(&dir, &url, &username, &password)
    })
    .await
    .map_err(|e| CoreError::Io {
        detail: format!("WebDAV task failed: {e}"),
    })??;
    let _ = vardo_core::recents::remember(&dir, &source);
    Ok(source)
}

// --- sync -------------------------------------------------------------------

#[tauri::command]
pub fn sync_preview(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> CmdResult<SyncPreview> {
    let dir = app_dir(&app)?;
    let ctx = source_context(&app)?;
    with_session(&state, |s| s.sync_preview(&dir, &ctx))
}

#[tauri::command]
pub fn sync_apply(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    policy: vardo_core::model::ConflictPolicy,
) -> CmdResult<SyncOutcome> {
    let dir = app_dir(&app)?;
    let ctx = source_context(&app)?;
    with_session_mut(&state, |s| s.sync_apply(&dir, &ctx, policy))
}

// --- saving -----------------------------------------------------------------

/// Writes the vault to disk.
///
/// Blocking, because saving does two full KDF passes — one to encrypt and one
/// to verify the result re-opens before anything is replaced. That is roughly
/// 1.7s on the reference vault, and doing it on the async runtime's thread
/// would freeze the window exactly as a synchronous unlock would.
#[tauri::command]
pub async fn save_vault(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> CmdResult<SaveResult> {
    let state = state.inner().clone();
    let ctx = source_context(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let mut guard = state.session.lock().unwrap();
        let session = guard.as_mut().ok_or(CoreError::Locked)?;
        session.save(&ctx)
    })
    .await
    .map_err(|e| CoreError::Io {
        detail: e.to_string(),
    })?
}

/// Everything needed to reach any vault source.
fn source_context(app: &tauri::AppHandle) -> CmdResult<vardo_core::source::SourceContext> {
    Ok(vardo_core::source::SourceContext {
        app_dir: app_dir(app)?,
        client: DriveClient::built_in(),
    })
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

// --- Google Drive -----------------------------------------------------------

#[tauri::command]
pub fn drive_status(app: tauri::AppHandle) -> CmdResult<vardo_core::model::DriveStatus> {
    let dir = app_dir(&app)?;
    Ok(vardo_core::model::DriveStatus {
        configured: DriveClient::built_in().is_some(),
        connected: vardo_core::drive::tokens::is_connected(&dir),
    })
}

/// Runs the OAuth flow, opening the user's real browser.
///
/// Blocking, because it waits for the redirect. Deliberately not the app's own
/// WebView: an embedded browser showing a third-party login is
/// indistinguishable from a phishing page, and the user has no address bar to
/// check it against.
#[tauri::command]
pub async fn drive_connect(app: tauri::AppHandle) -> CmdResult<()> {
    let dir = app_dir(&app)?;
    let client = DriveClient::built_in().ok_or(CoreError::Unsupported {
        detail: "this build has no Google client id, so Drive cannot be connected".into(),
    })?;

    let pending = vardo_core::drive::auth::begin(&client)?;

    // Hand the URL to the system browser. If that fails the user still gets it
    // in the event, so the flow is recoverable by copying it.
    let _ = open_in_browser(&pending.url);
    app.emit("drive://authorize-url", pending.url.clone())
        .map_err(|e| CoreError::Io {
            detail: e.to_string(),
        })?;

    tauri::async_runtime::spawn_blocking(move || {
        vardo_core::drive::auth::complete(&client, pending, &dir)
    })
    .await
    .map_err(|e| CoreError::Io {
        detail: e.to_string(),
    })?
}

#[tauri::command]
pub fn drive_disconnect(app: tauri::AppHandle) -> CmdResult<()> {
    vardo_core::drive::tokens::disconnect(&app_dir(&app)?)
}

#[tauri::command]
pub fn browser_hosts() -> CmdResult<vardo_core::model::BrowserHosts> {
    vardo_core::browser::host::status()
}

#[tauri::command]
pub fn browser_connect() -> CmdResult<vardo_core::model::BrowserHosts> {
    vardo_core::browser::host::connect()
}

#[tauri::command]
pub fn browser_disconnect() -> CmdResult<vardo_core::model::BrowserHosts> {
    vardo_core::browser::host::disconnect()
}

fn open_in_browser(url: &str) -> std::io::Result<()> {
    #[cfg(target_os = "linux")]
    let program = "xdg-open";
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(target_os = "windows")]
    let program = "explorer";

    std::process::Command::new(program)
        .arg(url)
        .spawn()
        .map(|_| ())
}

/// Vaults in Drive that Vardo is allowed to see.
///
/// Under the `drive.file` scope this is only what Vardo created or was handed,
/// so an empty result is a normal state rather than a failure. The UI says so.
#[tauri::command]
pub async fn drive_list_vaults(app: tauri::AppHandle) -> CmdResult<Vec<DriveVault>> {
    let dir = app_dir(&app)?;
    let client = DriveClient::built_in().ok_or(CoreError::Unsupported {
        detail: "this build has no Google client id".into(),
    })?;

    tauri::async_runtime::spawn_blocking(move || -> CmdResult<Vec<DriveVault>> {
        vardo_core::drive::vaults::list(&client, &dir).map(|files| {
            files
                .into_iter()
                .map(|f| DriveVault {
                    file_id: f.id,
                    name: f.name,
                    size: f.size,
                    modified_unix: f.modified_unix,
                })
                .collect()
        })
    })
    .await
    .map_err(|e| CoreError::Io {
        detail: e.to_string(),
    })?
}

/// Copies a local vault into Drive, leaving the original where it is.
#[tauri::command]
pub async fn drive_upload_vault(app: tauri::AppHandle, path: String) -> CmdResult<VaultSource> {
    let dir = app_dir(&app)?;
    let client = DriveClient::built_in().ok_or(CoreError::Unsupported {
        detail: "this build has no Google client id".into(),
    })?;

    // Blocking: uploading a 646 KB vault over a slow link is seconds, and on
    // the async runtime's thread that is a frozen window.
    let source = tauri::async_runtime::spawn_blocking(move || {
        vardo_core::drive::vaults::upload(&client, &dir, std::path::Path::new(&path))
    })
    .await
    .map_err(|e| CoreError::Io {
        detail: e.to_string(),
    })??;

    if let Ok(dir) = app_dir(&app) {
        let _ = vardo_core::recents::remember(&dir, &source);
    }
    Ok(source)
}

/// Opens Google's file picker so the user can hand Vardo an existing vault.
///
/// The only way to reach a file Vardo did not create without widening the
/// scope to the user's whole Drive. Returns `None` if they close it without
/// choosing, which is an ordinary outcome rather than a failure.
#[tauri::command]
pub async fn drive_pick_vault(app: tauri::AppHandle) -> CmdResult<Option<VaultSource>> {
    let dir = app_dir(&app)?;
    let client = DriveClient::built_in().ok_or(CoreError::Unsupported {
        detail: "this build has no Google client id".into(),
    })?;

    let pending = tauri::async_runtime::spawn_blocking({
        let dir = dir.clone();
        move || vardo_core::drive::picker::begin(&client, &dir)
    })
    .await
    .map_err(|e| CoreError::Io {
        detail: e.to_string(),
    })??;

    // The URL also goes to the UI, so the flow is recoverable by copying it if
    // opening a browser fails - which it does on minimal Linux installs.
    let _ = open_in_browser(&pending.url);
    app.emit("drive://picker-url", pending.url.clone())
        .map_err(|e| CoreError::Io {
            detail: e.to_string(),
        })?;

    let picked =
        tauri::async_runtime::spawn_blocking(move || vardo_core::drive::picker::wait(pending))
            .await
            .map_err(|e| CoreError::Io {
                detail: e.to_string(),
            })??;

    if let Some(source) = &picked {
        let _ = vardo_core::recents::remember(&dir, source);
    }
    Ok(picked)
}

/// Downloads this entry's site icon and stores it in the vault.
///
/// Blocking: it makes up to three HTTP requests. Returns null when the site
/// has no usable icon, which is ordinary rather than a failure. The session
/// stays free while the site answers, so other commands never wait on it.
#[tauri::command]
pub async fn fetch_icon(
    state: tauri::State<'_, AppState>,
    uuid: String,
) -> CmdResult<Option<String>> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        vardo_core::session::fetch_icon_shared(&state.session, &uuid)
    })
    .await
    .map_err(|e| CoreError::Io {
        detail: e.to_string(),
    })?
}

/// How many entries could get an icon, so the UI can say so before fetching.
#[tauri::command]
pub fn entries_missing_icons(state: tauri::State<'_, AppState>) -> CmdResult<Vec<String>> {
    with_session(&state, |s| Ok(s.entries_missing_icons()))
}

/// Writes an attachment to a path the user chose.
///
/// Returns the byte count so the UI can confirm rather than assume.
#[tauri::command]
pub fn save_attachment(
    state: tauri::State<'_, AppState>,
    uuid: String,
    name: String,
    dest: String,
) -> CmdResult<usize> {
    with_session(&state, |s| {
        s.save_attachment(&uuid, &name, std::path::Path::new(&dest))
    })
}

/// Reused, weak, expired and stale passwords.
#[tauri::command]
pub fn health(state: tauri::State<'_, AppState>) -> CmdResult<vardo_core::health::Report> {
    with_session(&state, |s| Ok(s.health()))
}

/// Earlier versions of an entry, newest first.
///
/// Carries no passwords, only whether each version has one. Listing must never
/// hand out a secret, and an old password is not a lesser secret: it is very
/// often still in use somewhere the user has forgotten about.
#[tauri::command]
pub fn history(
    state: tauri::State<'_, AppState>,
    uuid: String,
) -> CmdResult<Vec<vardo_core::model::HistoryVersion>> {
    with_session(&state, |s| s.history(&uuid))
}

/// Copies an old password, on the same clipboard-clearing schedule as any other.
#[tauri::command]
pub fn copy_history_password(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    uuid: String,
    index: usize,
) -> CmdResult<vardo_core::model::CopyResult> {
    let value = with_session(&state, |s| s.history_password(&uuid, index))?;
    crate::clipboard::copy_and_schedule_clear(&app, &value, vardo_core::CLIPBOARD_CLEAR_SECONDS)
        .map_err(|detail| CoreError::Io { detail })?;
    Ok(vardo_core::model::CopyResult {
        clear_after_seconds: vardo_core::CLIPBOARD_CLEAR_SECONDS,
    })
}

/// Whether a string would produce one-time codes.
///
/// Answered while it is being typed, because a mistyped seed produces codes
/// that look perfectly plausible and are simply wrong. You find that out when
/// you are locked out and in a hurry, with no way to tell whether the seed or
/// the clock is at fault.
///
/// Runs the same parser that will read it back, so the answer cannot drift
/// from what actually happens later. Takes no session: this is a question
/// about a string, and requiring an unlocked vault to ask it would be an odd
/// coupling.
#[tauri::command]
pub fn check_totp_seed(text: String) -> bool {
    vardo_core::totp::Totp::from_fields(Some(&text), None, None).is_some()
}

// --- groups -----------------------------------------------------------------

#[tauri::command]
pub fn create_group(
    state: tauri::State<'_, AppState>,
    parent: String,
    name: String,
) -> CmdResult<String> {
    with_session_mut(&state, |s| s.create_group(&parent, &name))
}

#[tauri::command]
pub fn move_group(
    state: tauri::State<'_, AppState>,
    uuid: String,
    parent: String,
) -> CmdResult<()> {
    with_session_mut(&state, |s| s.move_group(&uuid, &parent))
}

#[tauri::command]
pub fn rename_group(
    state: tauri::State<'_, AppState>,
    uuid: String,
    name: String,
) -> CmdResult<()> {
    with_session_mut(&state, |s| s.rename_group(&uuid, &name))
}

/// Removes a group and everything in it. Returns how many entries went with it.
#[tauri::command]
pub fn delete_group(state: tauri::State<'_, AppState>, uuid: String) -> CmdResult<usize> {
    with_session_mut(&state, |s| s.delete_group(&uuid))
}

/// How many entries a group holds, for the confirmation that precedes deleting
/// it. "Delete this group?" and "delete this group and the 38 passwords inside
/// it?" are different questions.
#[tauri::command]
pub fn group_entry_count(state: tauri::State<'_, AppState>, uuid: String) -> CmdResult<usize> {
    with_session(&state, |s| Ok(s.group_entry_count(&uuid)))
}

#[tauri::command]
pub fn set_expiry(
    state: tauri::State<'_, AppState>,
    uuid: String,
    at_unix: Option<i64>,
) -> CmdResult<()> {
    with_session_mut(&state, |s| s.set_expiry(&uuid, at_unix))
}

#[tauri::command]
pub fn set_tags(
    state: tauri::State<'_, AppState>,
    uuid: String,
    tags: Vec<String>,
) -> CmdResult<()> {
    with_session_mut(&state, |s| s.set_tags(&uuid, &tags))
}

/// Attaches a file. Returns the name it was stored under.
#[tauri::command]
pub fn add_attachment(
    state: tauri::State<'_, AppState>,
    uuid: String,
    path: String,
) -> CmdResult<String> {
    with_session_mut(&state, |s| {
        s.add_attachment(&uuid, std::path::Path::new(&path))
    })
}

#[tauri::command]
pub fn remove_attachment(
    state: tauri::State<'_, AppState>,
    uuid: String,
    name: String,
) -> CmdResult<()> {
    with_session_mut(&state, |s| s.remove_attachment(&uuid, &name))
}

/// Tells the tray whether the vault is open.
///
/// Called by the frontend rather than inferred from `AppState`, because the
/// frontend is what knows about the moments in between: a merge that rewrites
/// the vault, an auto-lock that fired, an unlock still deriving its key. The
/// tray must never claim the vault is locked while it is open, and the only
/// place that knows for certain is the thing driving the transitions.
#[tauri::command]
pub fn set_tray_unlocked(app: tauri::AppHandle, unlocked: bool) {
    crate::tray::set_unlocked(&app, unlocked);
}
