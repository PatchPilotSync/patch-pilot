//! Get Up To Speed on this install: for each machine, the scan its checklist
//! was made from and the steps the user marked done, kept in
//! `up-to-speed.json` in the app-data folder. Keeping the plan's starting
//! point lets later rescans tick steps off instead of making them vanish;
//! progress itself comes from [`patchpilot_parity::plan_progress`].

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const FILE: &str = "up-to-speed.json";
/// When this machine was last scanned, changed or not (Unix seconds).
const LAST_CHECK_FILE: &str = "last-check";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpToSpeedState {
    /// By machine ID.
    #[serde(default)]
    pub machines: BTreeMap<String, PlanStart>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanStart {
    /// The machine's snapshot the checklist was made from.
    pub snapshot: i64,
    /// The Golden Rig version and target profile it was made against; a
    /// change to either starts a fresh checklist.
    pub golden_version: u32,
    #[serde(default)]
    pub profile_id: Option<String>,
    /// Step keys (`step:<n>`) the user marked done by hand.
    #[serde(default)]
    pub ticked: BTreeSet<String>,
}

/// The saved checklists; empty when there are none yet.
pub fn load_up_to_speed(app_dir: &Path) -> Result<UpToSpeedState, String> {
    let path = app_dir.join(FILE);
    match std::fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str(&s).map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(UpToSpeedState::default()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// Write the checklists to a temp name and rename it into place.
pub fn save_up_to_speed(app_dir: &Path, state: &UpToSpeedState) -> Result<(), String> {
    let json = serde_json::to_string_pretty(state).map_err(|e| e.to_string())?;
    write_atomic(app_dir, FILE, &(json + "\n"))
}

/// Remember that this machine was scanned at `now`, even when nothing
/// changed and no snapshot was added.
pub fn record_check(app_dir: &Path, now: i64) -> Result<(), String> {
    write_atomic(app_dir, LAST_CHECK_FILE, &format!("{now}\n"))
}

/// When this machine was last scanned (see [`record_check`]).
pub fn last_check(app_dir: &Path) -> Option<i64> {
    std::fs::read_to_string(app_dir.join(LAST_CHECK_FILE))
        .ok()?
        .trim()
        .parse()
        .ok()
}

fn write_atomic(app_dir: &Path, name: &str, text: &str) -> Result<(), String> {
    let path: PathBuf = app_dir.join(name);
    let tmp = app_dir.join(format!("{name}.tmp"));
    std::fs::create_dir_all(app_dir)
        .and_then(|()| std::fs::write(&tmp, text))
        .and_then(|()| std::fs::rename(&tmp, &path))
        .map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checklists_and_last_check_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("PatchPilot");
        assert_eq!(load_up_to_speed(&app).unwrap(), UpToSpeedState::default());
        assert_eq!(last_check(&app), None);

        let mut state = UpToSpeedState::default();
        state.machines.insert(
            "abc".into(),
            PlanStart {
                snapshot: 4,
                golden_version: 2,
                profile_id: Some("travel".into()),
                ticked: BTreeSet::from(["step:3".to_owned()]),
            },
        );
        save_up_to_speed(&app, &state).unwrap();
        assert_eq!(load_up_to_speed(&app).unwrap(), state);

        record_check(&app, 1_800_000_000).unwrap();
        assert_eq!(last_check(&app), Some(1_800_000_000));
    }
}
