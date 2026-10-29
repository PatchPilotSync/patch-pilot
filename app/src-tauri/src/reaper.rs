//! Reaper extras in Up To Speed (decision #52): restore ReaPack's
//! repositories and packages, and point Reaper's folder settings at this
//! machine's folders. Writes go through restore points, so they refuse while
//! Reaper is open and can be undone from History.

use patchpilot_local::reaper_ext::{self, ExtensionCheck, FixPlan};
use patchpilot_local::reaper_extras::{
    self, NewFile, ReaperExtras, folder_changes, reapack_changes,
};
use patchpilot_local::reaper_startup::{self, StartupCheck};
use patchpilot_local::restore::{ApplyError, ChangeSet, Daw, FileChange, apply_changes};
use patchpilot_parity::reaper_ext::FixAction;
use patchpilot_store::Store;
use tauri::{AppHandle, State};

use crate::rig::{Scanning, db_path, err, open, rescan_quietly};

/// What this machine needs to match the Primary Rig's ReaPack and folder
/// settings. `None` when there is no Primary Rig or no scan of this machine,
/// or this machine is the one the Primary Rig was made from.
#[tauri::command]
pub async fn reaper_extras() -> Result<Option<ReaperExtras>, String> {
    let db = db_path()?;
    if !db.exists() {
        return Ok(None);
    }
    let store = open(&db)?;
    load(&store)
}

/// Add Primary Rig's missing ReaPack repositories and packages.
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

/// Reaper's extensions here (decision #104): whether each one loads, and
/// the ones scripts here call that aren't installed. `None` when Reaper
/// isn't set up on this machine. Works on every machine, Primary Rig too.
#[tauri::command]
pub async fn reaper_extensions() -> Result<Option<ExtensionCheck>, String> {
    let Some(dir) = patchpilot_local::standard_reaper_dir() else {
        return Ok(None);
    };
    let db = db_path()?;
    let machine = match this_machine(&db)? {
        Some(id) if db.exists() => open(&db)?.latest_manifest(&id).map_err(err)?,
        _ => None,
    };
    Ok(Some(check_here(&dir, machine.as_ref())))
}

/// Make the fix the extensions check offers for `file` (below
/// `UserPlugins/`): unblock it, move it out of a subfolder, or remove an
/// older copy. Moves and removals are restore points (Reaper closed, undo
/// in History); unblocking only clears macOS's quarantine flag.
#[tauri::command]
pub async fn fix_reaper_extension(
    app: AppHandle,
    scanning: State<'_, Scanning>,
    file: String,
    action: FixAction,
) -> Result<Option<ExtensionCheck>, String> {
    let dir =
        patchpilot_local::standard_reaper_dir().ok_or("Reaper isn't set up on this machine")?;
    let os = patchpilot_core::Os::current().ok_or("unsupported OS")?;
    match reaper_ext::fix_plan(&dir, os, patchpilot_local::current_arch(), &file, action)? {
        FixPlan::Unblock(path) => reaper_ext::unblock(&path)?,
        FixPlan::Files(label, changes) => {
            let db = db_path()?;
            let mut store = open(&db)?;
            let id = this_machine(&db)?.ok_or("scan this machine first")?;
            let blobs =
                patchpilot_local::restore::default_blob_dir().ok_or("no app-data folder")?;
            let req = ChangeSet {
                machine_id: id,
                label,
                daw: Some(Daw::Reaper),
                changes,
                now: patchpilot_local::now(),
            };
            apply_changes(&mut store, &blobs, &req).map_err(|e: ApplyError| e.to_string())?;
        }
    }
    // Rescan so Compare and the checklist see it installed.
    if scanning.start() {
        let r = rescan_quietly(&app);
        scanning.finish();
        r?;
    }
    reaper_extensions().await
}

fn check_here(
    dir: &std::path::Path,
    machine: Option<&patchpilot_core::Manifest>,
) -> ExtensionCheck {
    let os = machine.map_or_else(
        || patchpilot_core::Os::current().unwrap_or(patchpilot_core::Os::Windows),
        |m| m.machine.os,
    );
    reaper_ext::check(dir, os, patchpilot_local::current_arch(), machine)
}

/// Reaper's startup action here (decision #108) against the Primary Rig's,
/// through this machine's profile and ignores. `None` when Reaper isn't set
/// up here. On the Primary Rig, and with the startup action turned off in
/// Choose what syncs, only this machine's own is checked.
#[tauri::command]
pub async fn reaper_startup() -> Result<Option<StartupCheck>, String> {
    let Some(dir) = patchpilot_local::standard_reaper_dir() else {
        return Ok(None);
    };
    let os = patchpilot_core::Os::current().ok_or("unsupported OS")?;
    let db = db_path()?;
    let mut primary = None;
    if let (Some(id), true) = (this_machine(&db)?, db.exists()) {
        let store = open(&db)?;
        let syncs = patchpilot_local::sync_prefs::load(&crate::sync::app_dir()?)
            .syncs("reaper/startup-action");
        if let Some(golden) = store.golden(None).map_err(err)?
            && syncs
            && golden.info.source_machine_id.as_deref() != Some(id.as_str())
        {
            let profile = store.target_for(&id).map_err(err)?;
            primary = Some(patchpilot_parity::apply_profile(&golden.manifest, &profile));
        }
    }
    Ok(Some(reaper_startup::check(&dir, os, primary.as_ref())))
}
