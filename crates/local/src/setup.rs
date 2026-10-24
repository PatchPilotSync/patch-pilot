//! New-machine setup on this machine: what was planned and ticked, kept in
//! `setup.json` in the app-data folder (never in user folders). Progress
//! itself is worked out from scans by [`patchpilot_parity::setup_progress`].

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const SETUP_FILE: &str = "setup.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetupState {
    pub machine_id: String,
    /// Unix seconds.
    pub started_at: i64,
    /// The machine's snapshot when setup started; the plan is made from it.
    pub baseline_snapshot: i64,
    /// The Golden Rig version setup compares against for its whole run.
    pub golden_version: u32,
    /// Step keys the user ticked (`check:...`) or marked done (`step:<n>`).
    #[serde(default)]
    pub ticked: BTreeSet<String>,
    #[serde(default)]
    pub finished_at: Option<i64>,
}

/// The setup in progress (or finished but not yet closed), if any.
pub fn load_setup(app_dir: &Path) -> Result<Option<SetupState>, String> {
    let path = setup_path(app_dir);
    match std::fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str(&s)
            .map(Some)
            .map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// Write the setup state to a temp name and rename it into place.
pub fn save_setup(app_dir: &Path, state: &SetupState) -> Result<(), String> {
    let path = setup_path(app_dir);
    let tmp = app_dir.join(format!("{SETUP_FILE}.tmp"));
    let json = serde_json::to_string_pretty(state).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(app_dir)
        .and_then(|()| std::fs::write(&tmp, json + "\n"))
        .and_then(|()| std::fs::rename(&tmp, &path))
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// Close the setup. Scans and the Golden Rig are untouched.
pub fn clear_setup(app_dir: &Path) -> Result<(), String> {
    let path = setup_path(app_dir);
    match std::fs::remove_file(&path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            Err(format!("{}: {e}", path.display()))
        }
        _ => Ok(()),
    }
}

fn setup_path(app_dir: &Path) -> PathBuf {
    app_dir.join(SETUP_FILE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_state_round_trips_and_clears() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("PatchPilot");
        assert_eq!(load_setup(&app).unwrap(), None);
        clear_setup(&app).unwrap();

        let mut state = SetupState {
            machine_id: "abc".into(),
            started_at: 10,
            baseline_snapshot: 3,
            golden_version: 2,
            ticked: BTreeSet::new(),
            finished_at: None,
        };
        save_setup(&app, &state).unwrap();
        state.ticked.insert("check:authorize".into());
        state.finished_at = Some(20);
        save_setup(&app, &state).unwrap();
        assert_eq!(load_setup(&app).unwrap(), Some(state));
        assert!(!app.join("setup.json.tmp").exists());

        clear_setup(&app).unwrap();
        assert_eq!(load_setup(&app).unwrap(), None);
    }
}
