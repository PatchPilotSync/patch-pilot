//! First run (#40): scan this machine, show "Here's your rig", offer to make
//! it the Golden Rig. Uses the same database and machine ID as the CLI.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use patchpilot_core::{Manifest, Os};
use patchpilot_local::{ScanOptions, Stage};
use patchpilot_parity::{Inventory, inventory};
use patchpilot_store::{Store, StoreError};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

/// Set while a scan runs, so a second click can't start another.
#[derive(Default)]
pub struct Scanning(AtomicBool);

impl Scanning {
    /// Claims the scanner; false when a scan is already running.
    pub fn start(&self) -> bool {
        !self.0.swap(true, Ordering::SeqCst)
    }

    pub fn finish(&self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// Emitted after a scan saved a snapshot that differs from the previous one,
/// whether the user or the background watcher started it. Live sync uploads
/// after it.
pub const RIG_CHANGED: &str = "rig-changed";

/// Everything the first-run screens show. Mirrored in `src/api.ts`.
#[derive(Debug, Serialize)]
pub struct Overview {
    /// This machine's latest saved scan, if any.
    pub machine: Option<MachineSummary>,
    pub inventory: Option<Inventory>,
    pub golden: Option<GoldenSummary>,
}

#[derive(Debug, Serialize)]
pub struct MachineSummary {
    pub id: String,
    pub name: String,
    pub os: Os,
    /// Unix seconds.
    pub scanned_at: i64,
}

#[derive(Debug, Serialize)]
pub struct GoldenSummary {
    pub version: u32,
    pub created_at: i64,
    /// True when the latest version was made from this machine.
    pub from_this_machine: bool,
}

#[derive(Debug, Serialize)]
pub struct ScanReport {
    pub overview: Overview,
    /// True when nothing changed since the last saved scan.
    pub unchanged: bool,
    pub warnings: Vec<String>,
}

/// Progress event payload (`scan-progress`).
#[derive(Debug, Clone, Serialize)]
pub struct Progress {
    pub step: usize,
    pub of: usize,
    pub label: &'static str,
}

const STEPS: usize = 4;

#[tauri::command]
pub async fn overview() -> Result<Overview, String> {
    let db = db_path()?;
    if !db.exists() {
        return Ok(Overview {
            machine: None,
            inventory: None,
            golden: None,
        });
    }
    let store = open(&db)?;
    let this = patchpilot_store::read_machine_id(patchpilot_local::app_dir(&db))
        .map_err(|e| e.to_string())?;
    load_overview(&store, this.as_deref())
}

/// Scan this machine with the standard folders and save the snapshot.
/// `name` names the machine on its first scan; later scans keep the saved name
/// unless a new one is given.
#[tauri::command]
pub async fn scan_machine(
    app: AppHandle,
    scanning: State<'_, Scanning>,
    name: Option<String>,
) -> Result<ScanReport, String> {
    if !scanning.start() {
        return Err("a scan is already running".to_owned());
    }
    let result = tauri::async_runtime::spawn_blocking(move || scan_blocking(&app, name, true))
        .await
        .map_err(|e| e.to_string())
        .and_then(|r| r);
    scanning.finish();
    result
}

/// Make this machine's latest saved scan Golden Rig v1.
#[tauri::command]
pub async fn make_golden(app: AppHandle) -> Result<Overview, String> {
    let db = db_path()?;
    let mut store = open(&db)?;
    let id = patchpilot_store::read_machine_id(patchpilot_local::app_dir(&db))
        .map_err(|e| e.to_string())?
        .ok_or("scan this machine first")?;
    if let Some(g) = store.golden(None).map_err(err)? {
        return Err(format!(
            "the Golden Rig already exists (v{})",
            g.info.version
        ));
    }
    let scan = store
        .latest_manifest(&id)
        .map_err(err)?
        .ok_or("scan this machine first")?;
    store
        .save_golden(
            &scan,
            Some(&id),
            Some("Made from the first scan"),
            patchpilot_local::now(),
        )
        .map_err(err)?;
    crate::sync::nudge(&app);
    load_overview(&store, Some(&id))
}

/// Rescan this machine without progress events, for the background watcher.
/// Does nothing (returns `None`) until the user has scanned it once. The
/// caller holds [`Scanning`].
pub(crate) fn rescan_quietly(app: &AppHandle) -> Result<Option<ScanReport>, String> {
    let db = db_path()?;
    if !db.exists() {
        return Ok(None);
    }
    let Some(id) = patchpilot_store::read_machine_id(patchpilot_local::app_dir(&db))
        .map_err(|e| e.to_string())?
    else {
        return Ok(None);
    };
    if open(&db)?.latest_manifest(&id).map_err(err)?.is_none() {
        return Ok(None);
    }
    scan_blocking(app, None, false).map(Some)
}

fn scan_blocking(
    app: &AppHandle,
    name: Option<String>,
    progress: bool,
) -> Result<ScanReport, String> {
    let emit = |step: usize, label: &'static str| {
        if !progress {
            return;
        }
        // Progress is best effort; a closed window shouldn't fail the scan.
        let _ = app.emit(
            "scan-progress",
            Progress {
                step,
                of: STEPS,
                label,
            },
        );
    };
    let db = db_path()?;
    let id = patchpilot_store::load_or_create_machine_id(patchpilot_local::app_dir(&db))
        .map_err(|e| e.to_string())?;
    let mut store = open(&db)?;
    let name = name
        .map(|n| n.trim().to_owned())
        .filter(|n| !n.is_empty())
        .or_else(|| store.machine(&id).ok().map(|m| m.name))
        .unwrap_or_else(|| "This machine".to_owned());

    let scan = patchpilot_local::scan_this_machine(ScanOptions::standard(id.clone(), name), |s| {
        let step = match s {
            Stage::VendorApps => 1,
            Stage::Plugins => 2,
            Stage::Templates => 3,
        };
        emit(step, s.label());
    })?;
    emit(4, "Saving the snapshot");
    let info = patchpilot_local::save_scan(&mut store, &scan.manifest).map_err(err)?;
    if !info.unchanged {
        let _ = app.emit(RIG_CHANGED, ());
    }
    Ok(ScanReport {
        overview: load_overview(&store, Some(&id))?,
        unchanged: info.unchanged,
        warnings: scan.warnings,
    })
}

fn load_overview(store: &Store, this: Option<&str>) -> Result<Overview, String> {
    let latest: Option<Manifest> = match this {
        Some(id) => store.latest_manifest(id).map_err(err)?,
        None => None,
    };
    let scanned_at = match (this, &latest) {
        (Some(id), Some(_)) => scanned_at(store, id)?,
        _ => 0,
    };
    let golden = store.golden(None).map_err(err)?.map(|g| GoldenSummary {
        version: g.info.version,
        created_at: g.info.created_at,
        from_this_machine: this.is_some() && g.info.source_machine_id.as_deref() == this,
    });
    Ok(Overview {
        inventory: latest.as_ref().map(inventory),
        machine: latest.map(|m| MachineSummary {
            id: m.machine.id,
            name: m.machine.name,
            os: m.machine.os,
            scanned_at,
        }),
        golden,
    })
}

/// When the machine's latest snapshot was taken (Unix seconds), 0 if never.
pub(crate) fn scanned_at(store: &Store, id: &str) -> Result<i64, String> {
    Ok(store
        .snapshots(id)
        .map_err(err)?
        .first()
        .map_or(0, |s| s.created_at))
}

pub(crate) fn db_path() -> Result<PathBuf, String> {
    patchpilot_local::default_db_path()
}

pub(crate) fn open(db: &Path) -> Result<Store, String> {
    Store::open(db).map_err(|e| format!("{}: {e}", db.display()))
}

pub(crate) fn err(e: StoreError) -> String {
    e.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Manifest {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/parity/plan/golden.json");
        Manifest::from_json(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn overview_of_a_saved_machine_and_its_golden() {
        let m = fixture();
        let id = m.machine.id.clone();
        let mut store = Store::open_in_memory().unwrap();

        let empty = load_overview(&store, Some(&id)).unwrap();
        assert!(empty.machine.is_none() && empty.golden.is_none());

        let snap = patchpilot_local::save_scan(&mut store, &m).unwrap();
        let o = load_overview(&store, Some(&id)).unwrap();
        let machine = o.machine.unwrap();
        assert_eq!(machine.name, m.machine.name);
        assert!(machine.scanned_at > 0, "{snap:?}");
        assert_eq!(o.inventory.unwrap().products, 6);
        assert!(o.golden.is_none());

        store.save_golden(&m, Some(&id), None, 5).unwrap();
        let g = load_overview(&store, Some(&id)).unwrap().golden.unwrap();
        assert_eq!((g.version, g.from_this_machine), (1, true));
        let other = load_overview(&store, Some("someone-else")).unwrap();
        assert!(other.machine.is_none());
        assert!(!other.golden.unwrap().from_this_machine);
    }

    #[test]
    fn overview_serializes_for_the_ui() {
        let m = fixture();
        let mut store = Store::open_in_memory().unwrap();
        patchpilot_local::save_scan(&mut store, &m).unwrap();
        let json =
            serde_json::to_value(load_overview(&store, Some(&m.machine.id)).unwrap()).unwrap();
        assert_eq!(json["machine"]["os"], "windows");
        assert_eq!(json["inventory"]["formats"][0]["format"], "vst3");
        assert_eq!(
            json["inventory"]["vendors"][0]["products"][0]["confidence"],
            "high"
        );
        assert_eq!(json["inventory"]["apps"][0]["kind"], "license_tool");
    }
}
