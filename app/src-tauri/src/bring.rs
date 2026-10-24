//! Bring over (decision #42): the Golden Rig's templates, scripts and themes
//! that this machine lacks, fetched from the relay and written with a backup.
//! What to bring and how it's written live in `patchpilot_local::bring`.

use std::collections::BTreeMap;

use patchpilot_local::bring::{self, FileToBring};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::rig::{Scanning, db_path, err, open};
use crate::sync::{app_dir, fetch_file, load_rig, this_machine};

/// Mirrored in `src/api.ts`.
#[derive(Debug, Serialize)]
pub struct BringView {
    /// Files only travel between linked machines.
    pub linked: bool,
    /// Sorted by app, then path.
    pub files: Vec<FileToBring>,
}

/// Mirrored in `src/api.ts`.
#[derive(Debug, Default, Serialize)]
pub struct BringResult {
    pub written: usize,
    pub backed_up: usize,
    pub backup_dir: Option<String>,
    /// Paths no linked machine has uploaded yet (open Patch Pilot on the
    /// Golden machine), or too large to travel.
    pub unavailable: Vec<String>,
}

/// What this machine could bring over from the Golden Rig.
#[tauri::command]
pub async fn files_to_bring() -> Result<BringView, String> {
    let linked = load_rig()?.is_some();
    let db = db_path()?;
    if !db.exists() {
        return Ok(BringView {
            linked,
            files: Vec::new(),
        });
    }
    Ok(BringView {
        linked,
        files: list(&this_machine()?)?,
    })
}

/// Bring over the chosen files (`root/rel` keys as listed), then rescan.
#[tauri::command]
pub async fn bring_files(
    app: AppHandle,
    scanning: State<'_, Scanning>,
    keys: Vec<String>,
) -> Result<BringResult, String> {
    let rig = load_rig()?.ok_or("Link this machine first, so it can fetch the files.")?;
    let this = this_machine()?;
    let machine = open(&db_path()?)?.latest_manifest(&this).map_err(err)?;
    let chosen: Vec<FileToBring> = list(&this)?
        .into_iter()
        .filter(|f| keys.contains(&key(f)))
        .collect();

    let mut result = BringResult::default();
    // (root, app) to (rel, bytes) of each file to write there.
    let mut by_root: BTreeMap<(String, String), Vec<Fetched>> = BTreeMap::new();
    for f in chosen {
        if f.too_large {
            result.unavailable.push(f.rel);
            continue;
        }
        match fetch_file(&rig, &f.hash).await? {
            Some(bytes) => by_root
                .entry((f.root, f.app))
                .or_default()
                .push((f.rel, bytes)),
            None => result.unavailable.push(f.rel),
        }
    }
    let backups = bring::backup_dir(&app_dir()?, patchpilot_local::now());
    for ((root, app_name), files) in by_root {
        let dir = bring::root_dir(machine.as_ref(), &root).ok_or_else(|| {
            format!("Can't find {app_name}'s folder on this machine. Open {app_name} once, then try again.")
        })?;
        let report = bring::write_files(&root, &dir, &backups, &files)?;
        result.written += report.written;
        result.backed_up += report.backed_up;
        if let Some(d) = report.backup_dir {
            result.backup_dir = Some(d.to_string_lossy().into_owned());
        }
    }

    // Rescan so Compare shows the new files at once; if a scan is already
    // running, the folder watcher picks them up.
    if result.written > 0 && scanning.start() {
        let rescanned = tauri::async_runtime::spawn_blocking({
            let app = app.clone();
            move || crate::rig::rescan_quietly(&app)
        })
        .await;
        scanning.finish();
        rescanned.map_err(|e| e.to_string())??;
        let _ = app.emit("data-changed", ());
    }
    Ok(result)
}

fn list(this: &str) -> Result<Vec<FileToBring>, String> {
    let store = open(&db_path()?)?;
    let Some(golden) = store.golden(None).map_err(err)? else {
        return Ok(Vec::new());
    };
    let machine = store.latest_manifest(this).map_err(err)?;
    let profile = store
        .machine_profile(this)
        .map_err(err)?
        .unwrap_or_default();
    Ok(bring::files_to_bring(
        &golden.manifest,
        &profile,
        machine.as_ref(),
    ))
}

type Fetched = (String, Vec<u8>);

fn key(f: &FileToBring) -> String {
    format!("{}/{}", f.root, f.rel)
}
