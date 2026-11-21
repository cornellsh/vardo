//! Whether, and how often, Vardo looks for a newer version of itself.
//!
//! Contract: nothing is checked until the person has said yes
//! (`inv:no-phone-home`). A missing or corrupt choice reads as `Unasked`, so
//! damage leads back to the question and never to a silent yes or no. A check
//! is due at most once a day.
//!
//! The check itself, the download and the signature all belong to the shell
//! (`dec:auto-update`); this module only decides and remembers.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use crate::CoreError;

/// Once a day finds a security fix within a day of its release, and is rare
/// enough that GitHub sees one request per running copy per day.
pub const INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

/// The person's answer to "check for updates automatically?".
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum UpdateChecks {
    #[default]
    Unasked,
    On,
    Off,
}

/// Where looking for an update stands, for the interface to show.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum UpdateState {
    Idle,
    Checking,
    UpToDate,
    /// The update is downloading and its signature not yet checked.
    Downloading {
        version: String,
    },
    /// Downloaded and its signature verified; installing needs a restart.
    Ready {
        version: String,
        notes: String,
    },
    Failed {
        message: String,
    },
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    /// The running version.
    pub current: String,
    pub checks: UpdateChecks,
    pub state: UpdateState,
}

#[derive(Serialize, Deserialize)]
struct Stored {
    checks: UpdateChecks,
}

fn store_path(app_dir: &Path) -> PathBuf {
    app_dir.join("updates.json")
}

pub fn checks(app_dir: &Path) -> UpdateChecks {
    std::fs::read_to_string(store_path(app_dir))
        .ok()
        .and_then(|text| serde_json::from_str::<Stored>(&text).ok())
        .map(|s| s.checks)
        .unwrap_or_default()
}

pub fn set_checks(app_dir: &Path, checks: UpdateChecks) -> Result<(), CoreError> {
    std::fs::create_dir_all(app_dir).map_err(|e| CoreError::Io {
        detail: e.to_string(),
    })?;
    let json = serde_json::to_vec(&Stored { checks }).map_err(|e| CoreError::Io {
        detail: e.to_string(),
    })?;
    crate::secrets::write_private(&store_path(app_dir), &json)
}

/// Whether to ask now. `last` is the previous check in this run, if any.
///
/// A clock that went backwards counts as due, so a wrong clock costs one extra
/// request instead of silencing checks until the clock catches up.
pub fn due(checks: UpdateChecks, last: Option<SystemTime>, now: SystemTime) -> bool {
    checks == UpdateChecks::On
        && last.is_none_or(|t| now.duration_since(t).map_or(true, |d| d >= INTERVAL))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vardo-updates-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn nothing_is_checked_until_the_person_says_yes() {
        let now = SystemTime::now();
        assert!(!due(UpdateChecks::Unasked, None, now));
        assert!(!due(UpdateChecks::Off, None, now));
        assert!(due(UpdateChecks::On, None, now));
    }

    #[test]
    fn a_check_is_due_once_a_day_and_after_the_clock_goes_back() {
        let now = SystemTime::now();
        let hour = Duration::from_secs(3600);
        assert!(!due(UpdateChecks::On, Some(now - hour), now));
        assert!(due(UpdateChecks::On, Some(now - INTERVAL), now));
        assert!(due(UpdateChecks::On, Some(now + hour), now));
    }

    #[test]
    fn the_answer_is_remembered_and_damage_leads_back_to_the_question() {
        let dir = tmp("remember");
        assert_eq!(checks(&dir), UpdateChecks::Unasked);
        set_checks(&dir, UpdateChecks::On).unwrap();
        assert_eq!(checks(&dir), UpdateChecks::On);
        set_checks(&dir, UpdateChecks::Off).unwrap();
        assert_eq!(checks(&dir), UpdateChecks::Off);
        std::fs::write(store_path(&dir), "{not json").unwrap();
        assert_eq!(checks(&dir), UpdateChecks::Unasked);
    }
}
