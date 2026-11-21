//! Updating Vardo itself (`dec:auto-update`).
//!
//! Core decides whether a check is due (`vardo_core::updates`); this module
//! asks GitHub, downloads, and installs. The updater plugin checks the
//! download against the public key in `tauri.conf.json` and the version the
//! signature names, so a swapped file or manifest is refused before anything
//! is shown as ready.
//!
//! The update is downloaded as soon as it is found and installed only when the
//! person clicks restart, because restarting locks the vault.

use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};
use vardo_core::updates::{self, UpdateChecks, UpdateState, UpdateStatus};
use vardo_core::CoreError;

use crate::commands::CmdResult;
use crate::AppState;

/// How often the background thread asks core whether a check is due. Short,
/// so a laptop that slept through the day checks soon after it wakes.
const TICK: Duration = Duration::from_secs(60 * 60);

#[derive(Default)]
pub struct Updates(Mutex<Inner>);

struct Inner {
    state: UpdateState,
    last_check: Option<SystemTime>,
    /// The verified download, kept until restart.
    ready: Option<(Update, Vec<u8>)>,
}

impl Default for Inner {
    fn default() -> Self {
        Self {
            state: UpdateState::Idle,
            last_check: None,
            ready: None,
        }
    }
}

pub fn start(app: AppHandle) {
    app.manage(Updates::default());
    std::thread::spawn(move || loop {
        let due = updates::due(
            checks(&app),
            app.state::<Updates>().0.lock().unwrap().last_check,
            SystemTime::now(),
        );
        if due {
            tauri::async_runtime::block_on(check(&app));
        }
        std::thread::sleep(TICK);
    });
}

fn checks(app: &AppHandle) -> UpdateChecks {
    app.path()
        .app_config_dir()
        .map(|dir| updates::checks(&dir))
        .unwrap_or_default()
}

fn status(app: &AppHandle) -> UpdateStatus {
    UpdateStatus {
        current: app.package_info().version.to_string(),
        checks: checks(app),
        state: app.state::<Updates>().0.lock().unwrap().state.clone(),
    }
}

fn set_state(app: &AppHandle, state: UpdateState) {
    app.state::<Updates>().0.lock().unwrap().state = state;
    let _ = app.emit("vardo://update", status(app));
}

/// One check, then the download if there is something newer. Does nothing if
/// a check is already running or an update is already waiting.
async fn check(app: &AppHandle) {
    {
        let updates = app.state::<Updates>();
        let mut inner = updates.0.lock().unwrap();
        if matches!(
            inner.state,
            UpdateState::Checking | UpdateState::Downloading { .. } | UpdateState::Ready { .. }
        ) {
            return;
        }
        inner.last_check = Some(SystemTime::now());
    }
    set_state(app, UpdateState::Checking);
    let state = match find_and_download(app).await {
        Ok(None) => UpdateState::UpToDate,
        Ok(Some((update, bytes))) => {
            let state = UpdateState::Ready {
                version: update.version.clone(),
                notes: update.body.clone().unwrap_or_default(),
            };
            app.state::<Updates>().0.lock().unwrap().ready = Some((update, bytes));
            state
        }
        Err(e) => {
            eprintln!("vardo: update check failed: {e}");
            UpdateState::Failed {
                message: e.to_string(),
            }
        }
    };
    set_state(app, state);
}

async fn find_and_download(
    app: &AppHandle,
) -> tauri_plugin_updater::Result<Option<(Update, Vec<u8>)>> {
    let updater = app
        .updater_builder()
        .timeout(Duration::from_secs(120))
        .build()?;
    let Some(update) = updater.check().await? else {
        return Ok(None);
    };
    set_state(
        app,
        UpdateState::Downloading {
            version: update.version.clone(),
        },
    );
    // `download` verifies the signature before it returns the bytes.
    let bytes = update.download(|_, _| {}, || {}).await?;
    Ok(Some((update, bytes)))
}

#[tauri::command]
pub fn update_status(app: AppHandle) -> UpdateStatus {
    status(&app)
}

/// Saves the answer, and checks straight away when it is yes.
#[tauri::command]
pub async fn set_update_checks(app: AppHandle, checks: UpdateChecks) -> CmdResult<UpdateStatus> {
    let dir = app.path().app_config_dir().map_err(|e| CoreError::Io {
        detail: e.to_string(),
    })?;
    updates::set_checks(&dir, checks)?;
    if checks == UpdateChecks::On {
        check(&app).await;
    }
    Ok(status(&app))
}

/// "Check now". Asking is consent for this one request, whatever the setting.
#[tauri::command]
pub async fn check_for_update(app: AppHandle) -> UpdateStatus {
    check(&app).await;
    status(&app)
}

/// Installs the waiting update and restarts.
///
/// Holds the session for the whole install, so no save can be half written
/// when the process ends, and refuses while there are unsaved changes, which a
/// restart would throw away.
#[tauri::command]
pub fn install_update(app: AppHandle, state: tauri::State<'_, AppState>) -> CmdResult<()> {
    let session = state.session.lock().unwrap();
    if session.as_ref().is_some_and(|s| s.is_dirty()) {
        return Err(CoreError::UnsavedChanges);
    }
    let updates = app.state::<Updates>();
    let inner = updates.0.lock().unwrap();
    let Some((update, bytes)) = inner.ready.as_ref() else {
        return Err(CoreError::Io {
            detail: "No update is waiting.".into(),
        });
    };
    // On Windows this starts the installer and exits the process.
    update.install(bytes).map_err(|e| CoreError::Io {
        detail: e.to_string(),
    })?;
    app.restart()
}
