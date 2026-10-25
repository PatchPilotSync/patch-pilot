//! New-machine setup (strategy section 17): a guided plan for a fresh
//! computer with live progress from rescans, and a setup report at the end.
//! The logic is in `patchpilot_parity::setup_progress`; this keeps the state
//! file and the report file.

use std::path::PathBuf;

use patchpilot_local::setup::{SetupState, clear_setup, load_setup, save_setup};
use patchpilot_parity::{ReportInfo, SetupProgress, compare, setup_progress, setup_report};
use patchpilot_store::Store;
use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::compare::ProfileRef;
use crate::rig::{db_path, err, open};

/// Mirrored by `SetupView` in `src/api.ts`.
#[derive(Debug, Serialize)]
pub struct SetupView {
    /// Suggest a guided setup: this machine has a scan, isn't where the
    /// Primary Rig came from, isn't at parity and has no setup running.
    pub offer: bool,
    /// Target profiles to choose from when starting.
    pub profiles: Vec<ProfileRef>,
    /// This machine's current target profile.
    pub profile_id: Option<String>,
    pub active: Option<ActiveSetup>,
}

#[derive(Debug, Serialize)]
pub struct ActiveSetup {
    pub machine: String,
    pub profile: Option<String>,
    pub golden_version: u32,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub progress: SetupProgress,
}

#[tauri::command]
pub async fn setup_view() -> Result<SetupView, String> {
    let (store, dir, this) = context()?;
    view(&store, &dir, this.as_deref())
}

/// Start setting up this machine against the latest Primary Rig, through
/// `profile_id` (None for the whole Primary Rig).
#[tauri::command]
pub async fn setup_start(profile_id: Option<String>) -> Result<SetupView, String> {
    let (mut store, dir, this) = context()?;
    let this = this.ok_or("scan this machine first")?;
    start(
        &mut store,
        &dir,
        &this,
        profile_id.as_deref(),
        patchpilot_local::now(),
    )?;
    view(&store, &dir, Some(&this))
}

/// Tick a final check, or mark a plan step done by hand (`done` false undoes it).
#[tauri::command]
pub async fn setup_tick(key: String, done: bool) -> Result<SetupView, String> {
    let (store, dir, this) = context()?;
    let mut state = load_setup(&dir)?.ok_or(NO_SETUP)?;
    if done {
        state.ticked.insert(key);
    } else {
        state.ticked.remove(&key);
    }
    save_setup(&dir, &state)?;
    view(&store, &dir, this.as_deref())
}

/// Mark the setup finished; the report and the screen stay until it's closed.
#[tauri::command]
pub async fn setup_finish() -> Result<SetupView, String> {
    let (store, dir, this) = context()?;
    let mut state = load_setup(&dir)?.ok_or(NO_SETUP)?;
    state.finished_at.get_or_insert(patchpilot_local::now());
    save_setup(&dir, &state)?;
    view(&store, &dir, this.as_deref())
}

/// Close the setup screen. Scans, profiles and the Primary Rig stay as they are.
#[tauri::command]
pub async fn setup_close() -> Result<SetupView, String> {
    let (store, dir, this) = context()?;
    clear_setup(&dir)?;
    view(&store, &dir, this.as_deref())
}

/// The setup report as Markdown.
#[tauri::command]
pub async fn setup_report_text() -> Result<String, String> {
    let (store, dir, this) = context()?;
    report(&store, &dir, this.as_deref())
}

/// Save the setup report to the Downloads folder; returns its path.
#[tauri::command]
pub async fn setup_save_report(app: AppHandle) -> Result<String, String> {
    let (store, dir, this) = context()?;
    let text = report(&store, &dir, this.as_deref())?;
    let state = load_setup(&dir)?.ok_or(NO_SETUP)?;
    let name = store.machine(&state.machine_id).map_err(err)?.name;
    let downloads = app.path().download_dir().map_err(|e| e.to_string())?;
    let path = downloads.join(format!(
        "Patch Pilot setup report - {}.md",
        file_safe(&name)
    ));
    let tmp = downloads.join(".patch-pilot-setup-report.tmp");
    std::fs::write(&tmp, text)
        .and_then(|()| std::fs::rename(&tmp, &path))
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path.display().to_string())
}

const NO_SETUP: &str = "no setup in progress";

fn context() -> Result<(Store, PathBuf, Option<String>), String> {
    let db = db_path()?;
    let dir = patchpilot_local::app_dir(&db).to_path_buf();
    let this = patchpilot_store::read_machine_id(&dir).map_err(|e| e.to_string())?;
    Ok((open(&db)?, dir, this))
}

pub(crate) fn start(
    store: &mut Store,
    dir: &std::path::Path,
    this: &str,
    profile_id: Option<&str>,
    now: i64,
) -> Result<SetupState, String> {
    let golden = store
        .golden(None)
        .map_err(err)?
        .ok_or("no Primary Rig yet")?;
    let baseline = store
        .snapshots(this)
        .map_err(err)?
        .first()
        .ok_or("scan this machine first")?
        .id;
    store.set_machine_profile(this, profile_id).map_err(err)?;
    let state = SetupState {
        machine_id: this.to_owned(),
        started_at: now,
        baseline_snapshot: baseline,
        golden_version: golden.info.version,
        ticked: Default::default(),
        finished_at: None,
    };
    save_setup(dir, &state)?;
    Ok(state)
}

pub(crate) fn view(
    store: &Store,
    dir: &std::path::Path,
    this: Option<&str>,
) -> Result<SetupView, String> {
    let profiles = store
        .target_profiles()
        .map_err(err)?
        .into_iter()
        .map(|p| ProfileRef {
            id: p.profile.id,
            name: p.profile.name,
        })
        .collect();
    let Some(this) = this else {
        return Ok(SetupView {
            offer: false,
            profiles,
            profile_id: None,
            active: None,
        });
    };
    let scan = store.latest_manifest(this).map_err(err)?;
    let profile = match &scan {
        Some(_) => store.machine_profile(this).map_err(err)?,
        None => None,
    };
    let active = active(store, dir, this)?;
    let golden = store.golden(None).map_err(err)?;
    let offer = match (&golden, &scan, &active) {
        (Some(g), Some(scan), None) => {
            g.info.source_machine_id.as_deref() != Some(this)
                && compare(&g.manifest, &profile.clone().unwrap_or_default(), scan).score < 100
        }
        _ => false,
    };
    Ok(SetupView {
        offer,
        profiles,
        profile_id: profile.map(|p| p.id),
        active,
    })
}

fn active(store: &Store, dir: &std::path::Path, this: &str) -> Result<Option<ActiveSetup>, String> {
    let Some(state) = load_setup(dir)? else {
        return Ok(None);
    };
    if state.machine_id != this {
        // Left over from another machine ID (a copied app-data folder).
        return Ok(None);
    }
    let golden = store
        .golden(Some(state.golden_version))
        .map_err(err)?
        .ok_or_else(|| format!("Primary Rig v{} is gone", state.golden_version))?;
    let baseline = store
        .snapshot_manifest(state.baseline_snapshot)
        .map_err(err)?;
    let machine = store
        .latest_manifest(this)
        .map_err(err)?
        .ok_or("no saved scan of this machine")?;
    let profile = store.machine_profile(this).map_err(err)?;
    let progress = setup_progress(
        &golden.manifest,
        &profile.clone().unwrap_or_default(),
        &baseline,
        &machine,
        &state.ticked,
    );
    Ok(Some(ActiveSetup {
        machine: machine.machine.name,
        profile: profile.map(|p| p.name),
        golden_version: state.golden_version,
        started_at: state.started_at,
        finished_at: state.finished_at,
        progress,
    }))
}

fn report(store: &Store, dir: &std::path::Path, this: Option<&str>) -> Result<String, String> {
    let this = this.ok_or(NO_SETUP)?;
    let a = active(store, dir, this)?.ok_or(NO_SETUP)?;
    let os = store.machine(this).map_err(err)?.os;
    Ok(setup_report(
        &ReportInfo {
            machine: &a.machine,
            os,
            profile: a.profile.as_deref(),
            golden_version: a.golden_version,
            started_at: a.started_at,
            finished_at: a.finished_at,
            now: patchpilot_local::now(),
        },
        &a.progress,
    ))
}

/// A machine name as part of a file name on Windows and macOS.
fn file_safe(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            c if c.is_control() => '-',
            c => c,
        })
        .collect();
    let s = s.trim().trim_end_matches('.').to_owned();
    if s.is_empty() {
        "this machine".to_owned()
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patchpilot_core::Manifest;

    fn load(rel: &str) -> Manifest {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/parity/plan")
            .join(rel);
        Manifest::from_json(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn new_laptop_is_offered_a_setup_and_follows_it() {
        let studio = load("golden.json");
        let laptop = load("laptop.json");
        let dir = std::env::temp_dir().join(format!("pp-setup-{}", patchpilot_local::random_id()));
        let mut store = Store::open_in_memory().unwrap();
        patchpilot_local::save_scan(&mut store, &studio).unwrap();
        store
            .save_golden(&studio, Some(&studio.machine.id), None, 1)
            .unwrap();
        patchpilot_local::save_scan(&mut store, &laptop).unwrap();
        let lap = laptop.machine.id.as_str();

        // The Primary Rig machine itself isn't offered a setup; the laptop is.
        assert!(!view(&store, &dir, Some(&studio.machine.id)).unwrap().offer);
        let v = view(&store, &dir, Some(lap)).unwrap();
        assert!(v.offer && v.active.is_none());
        assert!(v.profiles.iter().any(|p| p.id == "travel"));

        start(&mut store, &dir, lap, Some("studio-clone"), 100).unwrap();
        let v = view(&store, &dir, Some(lap)).unwrap();
        assert!(!v.offer);
        assert_eq!(v.profile_id.as_deref(), Some("studio-clone"));
        let a = v.active.unwrap();
        assert_eq!((a.machine.as_str(), a.golden_version), ("New Laptop", 1));
        assert_eq!(a.progress.done, 0);

        let text = report(&store, &dir, Some(lap)).unwrap();
        assert!(text.starts_with("# Setup report: New Laptop"), "{text}");
        assert!(text.contains("Studio clone"), "{text}");

        clear_setup(&dir).unwrap();
        assert!(view(&store, &dir, Some(lap)).unwrap().offer);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn file_names_are_safe() {
        assert_eq!(file_safe("Studio: A/B?"), "Studio- A-B-");
        assert_eq!(file_safe(" .. "), "this machine");
    }
}
