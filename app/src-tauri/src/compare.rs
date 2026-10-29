//! Comparison view (#41): every machine's score against the Primary Rig
//! through its target profile, the diff with reasons, the Up To Speed plan,
//! suggestions for the Primary Rig (M9), and importing another machine's
//! export file.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use patchpilot_core::{Bundle, Matcher, Os};
use patchpilot_local::ignore::IgnoreView;
use patchpilot_local::up_to_speed::{PlanStart, load_up_to_speed, save_up_to_speed};
use patchpilot_parity::{
    Comparison, ComparisonRow, Packages, Plan, SetupProgress, SuggestionGroup, SuggestionUnit,
    compare, plan_still_covers, suggestion_key, up_to_speed,
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
    /// True when the latest Primary Rig version was made from this machine.
    pub golden_source: bool,
    pub profile: Option<ProfileRef>,
    /// None until there is a Primary Rig.
    pub score: Option<u8>,
    /// Products here the Primary Rig lacks or has older, not ignored.
    pub suggestions: usize,
    /// The same, by kind: plug-ins, scripts, templates... in display order.
    pub suggestion_groups: Vec<(SuggestionGroup, usize)>,
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
    /// Product IDs of `comparison.rows` that could go into the Primary Rig
    /// (newer here or not in Primary Rig), without the ones the user ignored.
    pub suggested: Vec<String>,
    /// What each of `suggested` counts as, by product ID: a script with the
    /// rest of its folder, all of ReaPack as one.
    pub suggestion_units: BTreeMap<String, SuggestionUnit>,
    /// Rows that would be suggested but were ignored, to offer again.
    pub suggested_ignored: Vec<String>,
    /// What the user ignored on this machine, left out of `comparison`.
    pub ignores: Vec<IgnoreView>,
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
    let profile = store.target_for(&id).map_err(err)?;
    let (mut g, mut m) = (golden.manifest, scan);
    patchpilot_parity::align_products(&mut g, &mut m);
    Ok(up_to_speed(&g, &profile, &m))
}

/// Get Up To Speed for a machine, with each step ticked off once a rescan
/// shows it done. The checklist keeps the scan it was made from; a new one
/// starts when the Primary Rig or the machine's profile changes, or when
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
    let (latest, _) = store
        .latest_snapshot(id)
        .map_err(err)?
        .ok_or("no saved scan of that machine")?;
    let profile_id = store.machine_profile(id).map_err(err)?.map(|p| p.id);
    let profile = store.target_for(id).map_err(err)?;

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
            lines.push(format!("Primary Rig: added {}", v.join(", ")));
        }
        GoldenImport::Diverged => {
            lines.push("Primary Rig: both machines changed it, so nothing was imported".to_owned())
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

/// "Add to Primary Rig" on a suggestion: a new Primary Rig version taking these
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

/// "Update Primary Rig from this machine": `machine`'s whole latest scan
/// becomes the next Primary Rig version.
#[tauri::command]
pub async fn update_golden_from(app: AppHandle, machine: String) -> Result<u32, String> {
    let mut store = existing_store()?.ok_or("no saved scans yet")?;
    let v = patchpilot_local::golden::update_from(&mut store, &machine, patchpilot_local::now())?;
    changed_here(&app, &store);
    Ok(v.version)
}

/// "Ignore" in Compare, Get Up To Speed or Bring over: leave what `rules`
/// match out of `machine`'s target (this machine's when `None`, or every
/// machine's), so it stops being asked for and counted. Syncs to linked
/// machines.
#[tauri::command]
pub async fn ignore_items(
    app: AppHandle,
    machine: Option<String>,
    label: String,
    rules: Vec<Matcher>,
    every_machine: bool,
) -> Result<(), String> {
    let mut store = existing_store()?.ok_or("no saved scans yet")?;
    patchpilot_local::ignore::ignore(
        &mut store,
        &machine_or_this(machine)?,
        &label,
        rules,
        every_machine,
        patchpilot_local::now(),
    )?;
    changed_here(&app, &store);
    Ok(())
}

/// Right-click > Remove on a machine (decision #99): forget another machine
/// and its scans here. It stays gone until it scans again. This machine can't
/// be removed; its next scan would bring it straight back.
#[tauri::command]
pub async fn remove_machine(app: AppHandle, id: String) -> Result<(), String> {
    if this_machine()?.as_deref() == Some(id.as_str()) {
        return Err("this machine can't be removed from its own list".to_owned());
    }
    let mut store = existing_store()?.ok_or("no saved scans yet")?;
    store
        .remove_machine(&id, patchpilot_local::now())
        .map_err(err)?;
    let _ = app.emit("data-changed", ());
    Ok(())
}

/// Take back an ignore (by its key) on `machine` (this one when `None`):
/// it is asked for again.
#[tauri::command]
pub async fn unignore(app: AppHandle, machine: Option<String>, key: String) -> Result<(), String> {
    let mut store = existing_store()?.ok_or("no saved scans yet")?;
    let machine = machine_or_this(machine)?;
    patchpilot_local::ignore::unignore(&mut store, &machine, &key, patchpilot_local::now())?;
    changed_here(&app, &store);
    Ok(())
}

/// The ignores in force on `machine` (this one when `None`), newest first.
#[tauri::command]
pub async fn ignore_list(machine: Option<String>) -> Result<Vec<IgnoreView>, String> {
    let Some(store) = existing_store()? else {
        return Ok(Vec::new());
    };
    let golden = store.golden(None).map_err(err)?;
    patchpilot_local::ignore::views(
        &store,
        &machine_or_this(machine)?,
        golden.as_ref().map(|g| &g.manifest),
    )
}

fn machine_or_this(machine: Option<String>) -> Result<String, String> {
    match machine {
        Some(m) => Ok(m),
        None => this_machine()?.ok_or_else(|| "scan this machine first".to_owned()),
    }
}

/// Offer ignored suggestions for the Primary Rig again, on every linked machine.
#[tauri::command]
pub async fn unignore_suggestions(app: AppHandle, items: Vec<IgnoreItem>) -> Result<(), String> {
    let mut store = existing_store()?.ok_or("no saved scans yet")?;
    let now = patchpilot_local::now();
    for i in &items {
        store
            .set_suggestion_ignored(&i.product, i.version.as_deref(), false, now)
            .map_err(err)?;
    }
    changed_here(&app, &store);
    Ok(())
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

const NO_GOLDEN: &str = "no Primary Rig yet";

pub(crate) fn machines_view(store: &Store, this: Option<&str>) -> Result<MachinesView, String> {
    let golden = store.golden(None).map_err(err)?;
    let ignored = patchpilot_local::golden::ignored_keys(store)?;
    let mut machines = Vec::new();
    for info in store.machines().map_err(err)? {
        let profile = store.machine_profile(&info.id).map_err(err)?;
        let (score, suggestion_groups) =
            match (&golden, store.latest_manifest(&info.id).map_err(err)?) {
                (Some(g), Some(scan)) => {
                    let c = compare(
                        &g.manifest,
                        &store.target_for(&info.id).map_err(err)?,
                        &scan,
                    );
                    (
                        Some(c.score),
                        suggestion_groups(&c, &ignored, &Packages::of(&scan)),
                    )
                }
                _ => (None, Vec::new()),
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
            suggestions: suggestion_groups.iter().map(|(_, n)| n).sum(),
            suggestion_groups,
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
    let comparison = compare(&golden.manifest, &store.target_for(id).map_err(err)?, &scan);
    let ignored = patchpilot_local::golden::ignored_keys(store)?;
    let suggested = suggested(&comparison, &ignored);
    let packages = Packages::of(&scan);
    let suggestion_groups = suggestion_groups(&comparison, &ignored, &packages);
    let suggestion_units = open_suggestions(&comparison, &ignored)
        .map(|r| {
            (
                r.product_id.0.clone(),
                patchpilot_parity::suggestion_unit(r, &packages),
            )
        })
        .collect();
    let suggested_ignored = patchpilot_parity::suggestions(&comparison)
        .filter(|r| !suggested.contains(&r.product_id.0))
        .map(|r| r.product_id.0.clone())
        .collect();
    let ignores = patchpilot_local::ignore::views(store, id, Some(&golden.manifest))?;
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
            suggestions: suggestion_groups.iter().map(|(_, n)| n).sum(),
            suggestion_groups,
            id: info.id,
            name: info.name,
            os: info.os,
        },
        golden_version: golden.info.version,
        comparison,
        suggested,
        suggestion_units,
        suggested_ignored,
        ignores,
    })
}

/// Suggestions by kind, each script package or ReaPack counted once.
fn suggestion_groups(
    c: &Comparison,
    ignored: &BTreeSet<String>,
    packages: &Packages,
) -> Vec<(SuggestionGroup, usize)> {
    patchpilot_parity::suggestion_summary(open_suggestions(c, ignored), packages)
}

fn suggested(c: &Comparison, ignored: &BTreeSet<String>) -> Vec<String> {
    open_suggestions(c, ignored)
        .map(|r| r.product_id.0.clone())
        .collect()
}

/// Suggestions the user hasn't ignored.
fn open_suggestions<'a>(
    c: &'a Comparison,
    ignored: &'a BTreeSet<String>,
) -> impl Iterator<Item = &'a ComparisonRow> {
    patchpilot_parity::suggestions(c).filter(|r| {
        !ignored.contains(&suggestion_key(
            &r.product_id.0,
            r.machine_version.as_deref(),
        ))
    })
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
