//! Reaper extras in Up To Speed (decision #48): restore ReaPack's
//! repositories and packages, and point Reaper's folder settings at this
//! machine's folders. Writes go through restore points, so they refuse while
//! Reaper is open and can be undone from History.

use patchpilot_local::reaper_extras::{
    self, NewFile, ReaperExtras, folder_changes, reapack_changes,
};
use patchpilot_local::restore::{ApplyError, ChangeSet, Daw, FileChange, apply_changes};
use patchpilot_store::Store;
use tauri::{AppHandle, State};

use crate::rig::{Scanning, db_path, err, open, rescan_quietly};

/// What this machine needs to match the Golden Rig's ReaPack and folder
/// settings. `None` when there is no Golden Rig or no scan of this machine,
/// or this machine is the one Golden was made from.
#[tauri::command]
pub async fn reaper_extras() -> Result<Option<ReaperExtras>, String> {
    let db = db_path()?;
    if !db.exists() {
        return Ok(None);
    }
    let store = open(&db)?;
    load(&store)
}

/// Add Golden's missing ReaPack repositories and packages.
#[tauri::command]
pub async fn restore_reapack(
    app: AppHandle,
    scanning: State<'_, Scanning>,
) -> Result<Option<ReaperExtras>, String> {
    apply(&app, &scanning, |extras, dir| {
        let n = extras.reapack.missing_remotes.len() + extras.reapack.missing_packages.len();
        let files = reapack_changes(dir, &extras.reapack)?;
        Ok((format!("Restored {n} ReaPack item{}", plural(n)), files))
    })
}

/// Set Reaper's folder settings to the matching folders on this machine.
#[tauri::command]
pub async fn fix_reaper_folders(
    app: AppHandle,
    scanning: State<'_, Scanning>,
) -> Result<Option<ReaperExtras>, String> {
    apply(&app, &scanning, |extras, dir| {
        let n = extras
            .folders
            .iter()
            .filter(|f| f.proposed.is_some())
            .count();
        let files = folder_changes(dir, &extras.folders)?.into_iter().collect();
        Ok((format!("Set {n} Reaper folder{}", plural(n)), files))
    })
}

type Changes = (String, Vec<NewFile>);

fn apply(
    app: &AppHandle,
    scanning: &Scanning,
    changes: impl FnOnce(&ReaperExtras, &std::path::Path) -> Result<Changes, String>,
) -> Result<Option<ReaperExtras>, String> {
    let db = db_path()?;
    let mut store = open(&db)?;
    let id = this_machine(&db)?.ok_or("scan this machine first")?;
    let extras = load(&store)?.ok_or("nothing to restore")?;
    let dir =
        patchpilot_local::standard_reaper_dir().ok_or("Reaper isn't set up on this machine")?;
    let (label, files) = changes(&extras, &dir)?;
    if !files.is_empty() {
        let blobs = patchpilot_local::restore::default_blob_dir().ok_or("no app-data folder")?;
        let req = ChangeSet {
            machine_id: id,
            label,
            daw: Some(Daw::Reaper),
            changes: files
                .into_iter()
                .map(|f| FileChange {
                    path: f.path,
                    contents: Some(f.contents),
                })
                .collect(),
            now: patchpilot_local::now(),
        };
        apply_changes(&mut store, &blobs, &req).map_err(|e: ApplyError| e.to_string())?;
    }
    // Rescan so Compare and the plan see the change. Skipped if a scan is
    // already running; that scan picks it up.
    if scanning.start() {
        let r = rescan_quietly(app);
        scanning.finish();
        r?;
    }
    load(&open(&db)?)
}

fn load(store: &Store) -> Result<Option<ReaperExtras>, String> {
    let db = db_path()?;
    let Some(id) = this_machine(&db)? else {
        return Ok(None);
    };
    let Some(golden) = store.golden(None).map_err(err)? else {
        return Ok(None);
    };
    if golden.info.source_machine_id.as_deref() == Some(id.as_str()) {
        return Ok(None);
    }
    let Some(machine) = store.latest_manifest(&id).map_err(err)? else {
        return Ok(None);
    };
    let (mut g, mut m) = (golden.manifest, machine);
    patchpilot_parity::align_products(&mut g, &mut m);
    let dir = patchpilot_local::standard_reaper_dir();
    Ok(Some(reaper_extras::reaper_extras(&g, &m, dir.as_deref())))
}

fn this_machine(db: &std::path::Path) -> Result<Option<String>, String> {
    patchpilot_store::read_machine_id(patchpilot_local::app_dir(db)).map_err(|e| e.to_string())
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}
