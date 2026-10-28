//! This machine's app preferences from Settings (decision #97): when to
//! rescan, which notifications to show and when, the window at login, extra
//! plug-in folders, and how long to keep restore points. Kept in app-data
//! `app-prefs.json`, per machine, never synced. A missing or broken file is
//! the defaults, which are how Patch Pilot behaved before these existed.

use std::path::Path;

use patchpilot_core::PluginFormat;
use serde::{Deserialize, Serialize};

const PREFS_FILE: &str = "app-prefs.json";

/// When this machine rescans on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rescan {
    /// A little while after plug-in, template or preset folders change (decision #39).
    #[default]
    OnChanges,
    /// The same, and once a day too.
    Daily,
    /// Only when the user presses Rescan: no folder watching, no startup scan.
    Manual,
}

/// Hours of the day (local time, 0-23) when no notification shows. `from`
/// may be after `to`, e.g. 22 to 8 overnight; equal hours mean none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuietHours {
    pub from: u8,
    pub to: u8,
}

impl QuietHours {
    pub fn contains(self, hour: u8) -> bool {
        if self.from <= self.to {
            (self.from..self.to).contains(&hour)
        } else {
            hour >= self.from || hour < self.to
        }
    }
}

/// A plug-in folder to scan besides the standard ones.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtraFolder {
    pub format: PluginFormat,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppPrefs {
    pub rescan: Rescan,
    /// Rescan a few seconds after an update (decision #93).
    pub rescan_after_update: bool,
    /// Drift alerts' and keep-in-sync's OS notifications wait out these hours.
    pub quiet_hours: Option<QuietHours>,
    /// "Files kept in sync" notifications. Conflicts still show in the app.
    pub sync_notices: bool,
    /// Offer updates on their own; Check for Updates works either way.
    pub update_notices: bool,
    /// Open the window too when Patch Pilot starts at login.
    pub window_at_login: bool,
    pub extra_folders: Vec<ExtraFolder>,
    /// Forget restore points older than this many days; `None` keeps them all.
    pub restore_days: Option<u32>,
}

impl Default for AppPrefs {
    fn default() -> Self {
        Self {
            rescan: Rescan::OnChanges,
            rescan_after_update: true,
            quiet_hours: None,
            sync_notices: true,
            update_notices: true,
            window_at_login: false,
            extra_folders: Vec::new(),
            restore_days: None,
        }
    }
}

impl AppPrefs {
    /// Whether a notification may show at this local hour.
    pub fn may_notify(&self, hour: u8) -> bool {
        self.quiet_hours.is_none_or(|q| !q.contains(hour))
    }

    /// The extra folders as scan roots.
    pub fn extra_roots(&self) -> Vec<patchpilot_scanner::PluginRoot> {
        self.extra_folders
            .iter()
            .enumerate()
            .filter_map(|(n, f)| {
                let format = serde_json::to_value(f.format).ok()?;
                crate::extra_root(n + 1, &format!("{}={}", format.as_str()?, f.path)).ok()
            })
            .collect()
    }
}

pub fn load(app_dir: &Path) -> AppPrefs {
    std::fs::read_to_string(app_dir.join(PREFS_FILE))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Write all of them, staged then renamed.
pub fn save(app_dir: &Path, prefs: &AppPrefs) -> Result<(), String> {
    let path = app_dir.join(PREFS_FILE);
    let tmp = app_dir.join(format!("{PREFS_FILE}.tmp"));
    let json = serde_json::to_string_pretty(prefs).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(app_dir)
        .and_then(|()| std::fs::write(&tmp, json))
        .and_then(|()| std::fs::rename(&tmp, &path))
        .map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_when_missing_and_keeps_unknown_fields_out() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(load(tmp.path()), AppPrefs::default());
        std::fs::write(
            tmp.path().join(PREFS_FILE),
            r#"{"rescan":"daily","future":1}"#,
        )
        .unwrap();
        let prefs = load(tmp.path());
        assert_eq!(prefs.rescan, Rescan::Daily);
        assert!(prefs.rescan_after_update);
    }

    #[test]
    fn round_trips() {
        let tmp = tempfile::tempdir().unwrap();
        let prefs = AppPrefs {
            rescan: Rescan::Manual,
            quiet_hours: Some(QuietHours { from: 22, to: 8 }),
            extra_folders: vec![ExtraFolder {
                format: PluginFormat::Vst3,
                path: "D:/Plugins".into(),
            }],
            restore_days: Some(90),
            ..AppPrefs::default()
        };
        save(tmp.path(), &prefs).unwrap();
        assert_eq!(load(tmp.path()), prefs);
        assert_eq!(prefs.extra_roots().len(), 1);
    }

    #[test]
    fn quiet_hours_wrap_past_midnight() {
        let night = QuietHours { from: 22, to: 8 };
        assert!(night.contains(23) && night.contains(0) && night.contains(7));
        assert!(!night.contains(8) && !night.contains(21));
        let day = QuietHours { from: 9, to: 17 };
        assert!(day.contains(9) && !day.contains(17) && !day.contains(3));
        assert!(!QuietHours { from: 5, to: 5 }.contains(5));
        let prefs = AppPrefs {
            quiet_hours: Some(night),
            ..AppPrefs::default()
        };
        assert!(!prefs.may_notify(23) && prefs.may_notify(12));
    }
}
