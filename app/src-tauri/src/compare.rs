//! Comparison view (#41): every machine's score against the Golden Rig
//! through its target profile, the diff with reasons, the Up To Speed plan,
//! suggestions for the Golden Rig (M9), and importing another machine's
//! export file.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use patchpilot_core::{Bundle, Os, ProductId};
use patchpilot_local::up_to_speed::{PlanStart, load_up_to_speed, save_up_to_speed};
use patchpilot_parity::{
    Comparison, Plan, SetupProgress, compare, plan_still_covers, suggestion_key, up_to_speed,
};
use patchpilot_store::{GoldenImport, Store};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::rig::{db_path, err, open, scanned_at};

#[derive(Debug, Serialize)]
pub struct MachinesView {
    pub golden_version: Option<u32>,
    pub machines: Vec<MachineCard>,
}

#[derive(Debug, Serialize)]
pub struct MachineCard {
    pub id: String,
    pub name: String,
    pub os: Os,
    pub scanned_at: i64,
    pub is_this_machine: bool,
    /// True when the latest Golden version was made from this machine.
    pub golden_source: bool,
    pub profile: Option<ProfileRef>,
    /// None until there is a Golden Rig.
    pub score: Option<u8>,
    /// Products here the Golden Rig lacks or has older, not ignored.
    pub suggestions: usize,
}

#[derive(Debug, Serialize)]
pub struct ProfileRef {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct MachineComparison {
    pub machine: MachineCard,
    pub golden_version: u32,
    pub comparison: Comparison,
    /// Product IDs of `comparison.rows` that could go into the Golden Rig
    /// (newer here or not in Golden), without the ones the user ignored.
    pub suggested: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ImportSummary {
    /// Plain-language lines, e.g. "New scans: Travel MacBook".
    pub lines: Vec<String>,
}

#[tauri::command]
pub async fn machines() -> Result<MachinesView, String> {
    let Some(store) = existing_store()? else {
        return Ok(MachinesView {
            golden_version: None,
            machines: Vec::new(),
        });
    };
    machines_view(&store, this_machine()?.as_deref())
}

#[tauri::command]
pub async fn compare_machine(id: String) -> Result<MachineComparison, String> {
    let store = existing_store()?.ok_or("no saved scans yet")?;
    comparison(&store, &id, this_machine()?.as_deref())
}

#[tauri::command]
pub async fn plan_machine(id: String) -> Result<Plan, String> {
    let store = existing_store()?.ok_or("no saved scans yet")?;
    let golden = store.golden(None).map_err(err)?.ok_or(NO_GOLDEN)?;
    let scan = store
        .latest_manifest(&id)
        .map_err(err)?
        .ok_or("no saved scan of that machine")?;
    let profile = store.machine_profile(&id).map_err(err)?.unwrap_or_default();
    let (mut g, mut m) = (golden.manifest, scan);
    patchpilot_parity::align_products(&mut g, &mut m);
    Ok(up_to_speed(&g, &profile, &m))
}

/// Get Up To Speed for a machine, with each step ticked off once a rescan
/// shows it done. The checklist keeps the scan it was made from; a new one
/// starts when the Golden Rig or the machine's profile changes, or when
/// something new is missing.
#[tauri::command]
pub async fn plan_progress(id: String) -> Result<SetupProgress, String> {
    let db = db_path()?;
    let store = existing_store()?.ok_or("no saved scans yet")?;
    plan_view(&store, patchpilot_local::app_dir(&db), &id)
}

/// Mark a Get Up To Speed step done by hand (`done` false undoes it), for
/// things Patch Pilot can't see.
#[tauri::command]
pub async fn plan_tick(id: String, key: String, done: bool) -> Result<SetupProgress, String> {
    let db = db_path()?;
    let dir = patchpilot_local::app_dir(&db);
    let store = existing_store()?.ok_or("no saved scans yet")?;
    plan_view(&store, dir, &id)?;
    let mut state = load_up_to_speed(dir)?;
    if let Some(start) = state.machines.get_mut(&id) {
        if done {
            start.ticked.insert(key);
        } else {
            start.ticked.remove(&key);
        }
        save_up_to_speed(dir, &state)?;
    }
    plan_view(&store, dir, &id)
}

fn plan_view(store: &Store, dir: &Path, id: &str) -> Result<SetupProgress, String> {
    let golden = store.golden(None).map_err(err)?.ok_or(NO_GOLDEN)?;
    let machine = store
        .latest_manifest(id)
        .map_err(err)?
        .ok_or("no saved scan of that machine")?;
    let latest = store
        .snapshots(id)
        .map_err(err)?
        .first()
        .ok_or("no saved scan of that machine")?
        .id;
    let profile = store.machine_profile(id).map_err(err)?;
    let profile_id = profile.as_ref().map(|p| p.id.clone());
    let profile = profile.unwrap_or_default();

    let mut state = load_up_to_speed(dir)?;
    let kept = match state.machines.get(id) {
        Some(s) if s.golden_version == golden.info.version && s.profile_id == profile_id => {
            match store.snapshot_manifest(s.snapshot) {
                Ok(baseline)
                    if plan_still_covers(&golden.manifest, &profile, &baseline, &machine) =>
                {
                    Some((baseline, s.ticked.clone()))
                }
                _ => None,
            }
        }
        _ => None,
    };
    let (baseline, ticked) = match kept {
        Some(k) => k,
        None => {
            state.machines.insert(
                id.to_owned(),
                PlanStart {
                    snapshot: latest,
                    golden_version: golden.info.version,
                    profile_id,
                    ticked: BTreeSet::new(),
                },
            );
            save_up_to_speed(dir, &state)?;
            (machine.clone(), BTreeSet::new())
        }
    };
    Ok(patchpilot_parity::plan_progress(
        &golden.manifest,
        &profile,
        &baseline,
        &machine,
        &ticked,
    ))
}

/// Add another machine's `patchpilot export` file to this database.
#[tauri::command]
pub async fn import_bundle(path: PathBuf) -> Result<ImportSummary, String> {
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let bundle = Bundle::from_json(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let db = db_path()?;
    let mut store = open(&db)?;
    let report = store
        .import_bundle(&bundle, this_machine()?.as_deref(), patchpilot_local::now())
        .map_err(err)?;
    let mut lines = Vec::new();
    let names = |v: &[String]| v.join(", ");
    if !report.machines_added.is_empty() {
        lines.push(format!("New scans: {}", names(&report.machines_added)));
    }
    if !report.machines_unchanged.is_empty() {
        lines.push(format!(
            "Already up to date: {}",
            names(&report.machines_unchanged)
        ));
    }
    match report.golden {
        GoldenImport::NotInBundle | GoldenImport::UpToDate => {}
        GoldenImport::Imported(v) => {
            let v: Vec<_> = v.iter().map(|n| format!("v{n}")).collect();
            lines.push(format!("Golden Rig: added {}", v.join(", ")));
        }
        GoldenImport::Diverged => {
            lines.push("Golden Rig: both machines changed it, so nothing was imported".to_owned())
        }
    }
    if !report.profiles_added.is_empty() {
        lines.push(format!("New profiles: {}", names(&report.profiles_added)));
    }
    if lines.is_empty() {
        lines.push("Nothing new in that file".to_owned());
    }
    Ok(ImportSummary { lines })
}

/// "Add to Golden Rig" on a suggestion: a new Golden version taking these
/// products from `machine`. Every linked machine gets it through sync.
#[tauri::command]
pub async fn add_to_golden(
    app: AppHandle,
    machine: String,
    products: Vec<String>,
) -> Result<u32, String> {
    let mut store = existing_store()?.ok_or("no saved scans yet")?;
    let v = patchpilot_local::golden::add_to_golden(
        &mut store,
        &machine,
        &products,
        patchpilot_local::now(),
    )?;
    changed_here(&app, &store);
    Ok(v.version)
}

/// "Update Golden Rig from this machine": `machine`'s whole latest scan
/// becomes the next Golden version.
#[tauri::command]
pub async fn update_golden_from(app: AppHandle, machine: String) -> Result<u32, String> {
    let mut store = existing_store()?.ok_or("no saved scans yet")?;
    let v = patchpilot_local::golden::update_from(&mut store, &machine, patchpilot_local::now())?;
    changed_here(&app, &store);
    Ok(v.version)
}

/// One suggestion to ignore: a product and the machine's version of it.
#[derive(Debug, serde::Deserialize)]
pub struct IgnoreItem {
    pub product: String,
    pub version: Option<String>,
}

/// "Ignore" on suggestions: stop offering these versions of the products, on
/// every linked machine. Takes a list so ReaPack's thousands of scripts are
/// one call.
#[tauri::command]
pub async fn ignore_suggestions(app: AppHandle, items: Vec<IgnoreItem>) -> Result<(), String> {
    let mut store = existing_store()?.ok_or("no saved scans yet")?;
    let now = patchpilot_local::now();
    for i in &items {
        store
            .ignore_suggestion(&i.product, i.version.as_deref(), now)
            .map_err(err)?;
    }
    changed_here(&app, &store);
    Ok(())
}

/// After a change made on this machine: no alert about it here, upload it,
/// and refresh the windows.
fn changed_here(app: &AppHandle, store: &Store) {
    crate::alerts::rebaseline(app, store);
    crate::sync::nudge(app);
    let _ = app.emit("data-changed", ());
}

const NO_GOLDEN: &str = "no Golden Rig yet";

pub(crate) fn machines_view(store: &Store, this: Option<&str>) -> Result<MachinesView, String> {
    let golden = store.golden(None).map_err(err)?;
    let ignored = patchpilot_local::golden::ignored_keys(store)?;
    let mut machines = Vec::new();
    for info in store.machines().map_err(err)? {
        let profile = store.machine_profile(&info.id).map_err(err)?;
        let (score, suggestions) = match (&golden, store.latest_manifest(&info.id).map_err(err)?) {
            (Some(g), Some(scan)) => {
                let c = compare(&g.manifest, &profile.clone().unwrap_or_default(), &scan);
                (Some(c.score), suggestion_count(&suggested(&c, &ignored)))
            }
            _ => (None, 0),
        };
        machines.push(MachineCard {
            scanned_at: scanned_at(store, &info.id)?,
            is_this_machine: Some(info.id.as_str()) == this,
            golden_source: golden
                .as_ref()
                .is_some_and(|g| g.info.source_machine_id.as_deref() == Some(info.id.as_str())),
            profile: profile.map(|p| ProfileRef {
                id: p.id,
                name: p.name,
            }),
            score,
            suggestions,
            id: info.id,
            name: info.name,
            os: info.os,
        });
    }
    // This machine first, then by name (store order).
    machines.sort_by_key(|m| !m.is_this_machine);
    Ok(MachinesView {
        golden_version: golden.map(|g| g.info.version),
        machines,
    })
}

pub(crate) fn comparison(
    store: &Store,
    id: &str,
    this: Option<&str>,
) -> Result<MachineComparison, String> {
    let golden = store.golden(None).map_err(err)?.ok_or(NO_GOLDEN)?;
    let info = store.machine(id).map_err(err)?;
    let scan = store
        .latest_manifest(id)
        .map_err(err)?
        .ok_or_else(|| format!("no saved scan of {}", info.name))?;
    let profile = store.machine_profile(id).map_err(err)?;
    let comparison = compare(
        &golden.manifest,
        &profile.clone().unwrap_or_default(),
        &scan,
    );
    let suggested = suggested(&comparison, &patchpilot_local::golden::ignored_keys(store)?);
    Ok(MachineComparison {
        machine: MachineCard {
            scanned_at: scanned_at(store, id)?,
            is_this_machine: Some(id) == this,
            golden_source: golden.info.source_machine_id.as_deref() == Some(id),
            profile: profile.map(|p| ProfileRef {
                id: p.id,
                name: p.name,
            }),
            score: Some(comparison.score),
            suggestions: suggestion_count(&suggested),
            id: info.id,
            name: info.name,
            os: info.os,
        },
        golden_version: golden.info.version,
        comparison,
        suggested,
    })
}

/// How many suggestions to show: ReaPack's packages count as one.
fn suggestion_count(ids: &[String]) -> usize {
    let reapack = |id: &String| patchpilot_parity::reapack::is_reapack(&ProductId(id.clone()));
    ids.iter().filter(|id| !reapack(id)).count() + usize::from(ids.iter().any(reapack))
}

fn suggested(c: &Comparison, ignored: &BTreeSet<String>) -> Vec<String> {
    patchpilot_parity::suggestions(c)
        .filter(|r| {
            !ignored.contains(&suggestion_key(
                &r.product_id.0,
                r.machine_version.as_deref(),
            ))
        })
        .map(|r| r.product_id.0.clone())
        .collect()
}

pub(crate) fn existing_store() -> Result<Option<Store>, String> {
    let db = db_path()?;
    if !db.exists() {
        return Ok(None);
    }
    open(&db).map(Some)
}

fn this_machine() -> Result<Option<String>, String> {
    let db = db_path()?;
    patchpilot_store::read_machine_id(patchpilot_local::app_dir(&db)).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use patchpilot_core::Manifest;

    fn load(rel: &str) -> Manifest {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures")
            .join(rel);
        Manifest::from_json(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn up_to_speed_keeps_its_steps_and_ticks_them_off() {
        let studio = load("parity/plan/golden.json");
        let laptop = load("parity/plan/laptop.json");
        let dir = tempfile_dir();
        let mut store = Store::open_in_memory().unwrap();
        patchpilot_local::save_scan(&mut store, &studio).unwrap();
        store
            .save_golden(&studio, Some(&studio.machine.id), None, 1)
            .unwrap();
        patchpilot_local::save_scan(&mut store, &laptop).unwrap();
        let id = laptop.machine.id.as_str();

        let start = plan_view(&store, &dir, id).unwrap();
        assert!(start.total > 0);
        assert_eq!(start.done, 0);

        // Everything gets installed: same steps, all crossed off.
        let mut done = studio.clone();
        done.machine = laptop.machine.clone();
        patchpilot_local::save_scan(&mut store, &done).unwrap();
        let p = plan_view(&store, &dir, id).unwrap();
        assert_eq!((p.done, p.total), (start.total, start.total));
        assert!(p.complete);
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn tempfile_dir() -> PathBuf {
        std::env::temp_dir().join(format!("pp-uts-{}", patchpilot_local::random_id()))
    }

    #[test]
    fn imported_machine_gets_a_score_with_reasons() {
        let studio = load("parity/score/golden.json");
        let laptop = load("parity/score/laptop.json");
        let mut store = Store::open_in_memory().unwrap();
        patchpilot_local::save_scan(&mut store, &studio).unwrap();
        store
            .save_golden(&studio, Some(&studio.machine.id), None, 1)
            .unwrap();

        // The laptop arrives from an export file.
        let mut bundle = Bundle::new(2, Some("Laptop".into()));
        bundle.machines.push(patchpilot_core::BundleMachine {
            manifest: laptop.clone(),
            profile_id: None,
        });
        store
            .import_bundle(&bundle, Some(&studio.machine.id), 3)
            .unwrap();

        let view = machines_view(&store, Some(&studio.machine.id)).unwrap();
        assert_eq!(view.golden_version, Some(1));
        assert_eq!(view.machines.len(), 2);
        assert!(view.machines[0].is_this_machine);
        assert_eq!(view.machines[0].score, Some(100));
        let lap = &view.machines[1];
        assert_eq!(lap.name, laptop.machine.name);
        assert!(lap.score.unwrap() < 100);

        let c = comparison(&store, &laptop.machine.id, Some(&studio.machine.id)).unwrap();
        assert_eq!(c.machine.score, lap.score);
        assert!(!c.comparison.reasons.is_empty());

        // With Travel assigned, the laptop is at parity.
        store
            .set_machine_profile(&laptop.machine.id, Some("travel"))
            .unwrap();
        let c = comparison(&store, &laptop.machine.id, None).unwrap();
        assert_eq!(c.comparison.score, 100);
        assert_eq!(c.machine.profile.unwrap().name, "Travel");
    }
}
