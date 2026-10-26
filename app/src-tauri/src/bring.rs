//! Bring over (decision #45): the Primary Rig's templates, scripts and themes
//! that this machine lacks, fetched from the relay and written as a restore
//! point History can undo. A file comes with every file it needs, or not at
//! all (decision #62), and can also come from another linked machine.
//! What to bring and how it's written live in `patchpilot_local::bring`.

use std::collections::{BTreeMap, BTreeSet};

use patchpilot_core::{Manifest, Os, TargetProfile};
use patchpilot_local::bring::{self, FileToBring};
use patchpilot_local::ignore::IgnoreView;
use patchpilot_store::Store;
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::rig::{Scanning, db_path, err, open};
use crate::sync::{app_dir, fetch_file, load_rig, this_machine};

/// Mirrored in `src/api.ts`.
#[derive(Debug, Serialize)]
pub struct BringView {
    /// Files only travel between linked machines.
    pub linked: bool,
    /// There is something to bring from: a Primary Rig, or the machine asked for.
    pub has_source: bool,
    /// Sorted by app, then path.
    pub files: Vec<FileToBring>,
    /// What the user ignored on this machine, left out of `files`.
    pub ignored: Vec<FileToBring>,
    /// The ignores that hide `ignored`, to take back.
    pub ignores: Vec<IgnoreView>,
}

/// Mirrored in `src/api.ts`.
#[derive(Debug, Default, Serialize)]
pub struct BringResult {
    pub written: usize,
    /// Replaced files, saved first: History can undo the bring-over.
    pub backed_up: usize,
    /// Already the same here, so nothing to write.
    pub same: usize,
    /// Files brought because a chosen file needs them.
    pub companions: usize,
    /// Paths no linked machine has uploaded yet (open Patch Pilot on the
    /// machine they come from), or too large to travel.
    pub unavailable: Vec<String>,
    /// Paths left behind because a file they need is unavailable: they
    /// wouldn't work without it.
    pub held_back: Vec<String>,
}

/// What this machine could bring over from the Primary Rig, or from the
/// linked machine `from`.
#[tauri::command]
pub async fn files_to_bring(from: Option<String>) -> Result<BringView, String> {
    let linked = load_rig()?.is_some();
    let db = db_path()?;
    if !db.exists() {
        return Ok(BringView {
            linked,
            has_source: false,
            files: Vec::new(),
            ignored: Vec::new(),
            ignores: Vec::new(),
        });
    }
    let this = this_machine()?;
    let (source, files) = list(&this, from.as_deref())?;
    let (ignored, ignores) = match &source {
        Some(source) => {
            let store = open(&db)?;
            let scan = store.latest_manifest(&this).map_err(err)?;
            let (base, target) = profiles(&store, &this, from.as_deref())?;
            let (ignored, ignores) = patchpilot_local::ignore::bring_ignored(
                &store,
                &this,
                source,
                &base,
                &target,
                scan.as_ref(),
            )?;
            (
                bring::drop_same_here(source, scan.as_ref(), ignored),
                ignores,
            )
        }
        None => Default::default(),
    };
    Ok(BringView {
        linked,
        has_source: source.is_some(),
        files,
        ignored,
        ignores,
    })
}

/// Bring over the chosen files (`root/rel` keys as listed) and every file
/// they need, from the Primary Rig or the linked machine `from`, then rescan.
#[tauri::command]
pub async fn bring_files(
    app: AppHandle,
    scanning: State<'_, Scanning>,
    keys: Vec<String>,
    from: Option<String>,
) -> Result<BringResult, String> {
    let rig = load_rig()?.ok_or("Link this machine first, so it can fetch the files.")?;
    let this = this_machine()?;
    let machine = open(&db_path()?)?.latest_manifest(&this).map_err(err)?;
    let (source, list) = list(&this, from.as_deref())?;
    let source = source.ok_or("Nothing to bring over from: make a Primary Rig first.")?;
    let chosen = bring::with_needs(&list, &keys);

    let mut result = BringResult {
        companions: chosen.iter().filter(|f| !keys.contains(&f.key())).count(),
        ..BringResult::default()
    };
    let mut fetched: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut bad = BTreeSet::new();
    for f in &chosen {
        let bytes = if f.too_large {
            None
        } else {
            fetch_file(&rig, &f.hash).await?
        };
        match bytes {
            Some(bytes) => {
                fetched.insert(f.key(), bytes);
            }
            None => {
                bad.insert(f.key());
                result.unavailable.push(f.rel.clone());
            }
        }
    }
    // A script whose library didn't come would only fail when it runs.
    let held = bring::blocked(&list, &bad);

    // (root, app) to (rel, bytes) of each file to write there.
    let mut by_root: BTreeMap<(String, String), Vec<Fetched>> = BTreeMap::new();
    for f in chosen {
        if held.contains(&f.key()) {
            result.held_back.push(f.rel.clone());
            continue;
        }
        if let Some(bytes) = fetched.remove(&f.key()) {
            by_root
                .entry((f.root.clone(), f.app.clone()))
                .or_default()
                .push((f.rel.clone(), bytes));
        }
    }
    let from_name = if from.is_some() {
        source.machine.name.clone()
    } else {
        "the Primary Rig".to_owned()
    };
    let app_dir = app_dir()?;
    let blobs = app_dir.join("blobs");
    let mut store = open(&db_path()?)?;
    for ((root, app_name), files) in by_root {
        let dir = bring::root_dir(machine.as_ref(), &root).ok_or_else(|| {
            format!("Can't find {app_name}'s folder on this machine. Open {app_name} once, then try again.")
        })?;
        let files: Vec<Fetched> = match Os::current() {
            Some(os) => files
                .into_iter()
                .map(|(rel, bytes)| {
                    let bytes = bring::adapt_for_here(&source, &root, &rel, &dir, os, bytes);
                    (rel, bytes)
                })
                .collect(),
            None => files,
        };
        let label = format!(
            "Brought over {} {app_name} file{} from {from_name}",
            files.len(),
            if files.len() == 1 { "" } else { "s" }
        );
        let report = bring::write_files(
            &mut store,
            &blobs,
            &this,
            &label,
            &root,
            &dir,
            &files,
            patchpilot_local::now(),
        )?;
        result.written += report.written;
        result.backed_up += report.backed_up;
        result.same += report.same;
    }
    drop(store);

    // Rescan so Compare and this list show the new files at once, also when
    // nothing was written because the files were already here (the last scan
    // was older than them). If a scan is already running, the folder watcher
    // picks them up.
    if (result.written > 0 || result.same > 0) && scanning.start() {
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

/// The manifest files come from (the Primary Rig through this machine's
/// profile, or `from`'s latest scan) and what it has that this machine lacks.
fn list(this: &str, from: Option<&str>) -> Result<(Option<Manifest>, Vec<FileToBring>), String> {
    let store = open(&db_path()?)?;
    let machine = store.latest_manifest(this).map_err(err)?;
    let source = match from {
        Some(id) if id == this => return Err("That's this machine.".to_owned()),
        Some(id) => store.latest_manifest(id).map_err(err)?,
        None => store.golden(None).map_err(err)?.map(|g| g.manifest),
    };
    let Some(source) = source else {
        return Ok((None, Vec::new()));
    };
    let (_, profile) = profiles(&store, this, from)?;
    let files = bring::files_to_bring(&source, &profile, machine.as_ref());
    let files = bring::drop_same_here(&source, machine.as_ref(), files);
    Ok((Some(source), files))
}

/// What files come through, without and with what the user ignored here:
/// from the Primary Rig this machine's profile, from another machine all of it.
fn profiles(
    store: &Store,
    this: &str,
    from: Option<&str>,
) -> Result<(TargetProfile, TargetProfile), String> {
    let base = match from {
        Some(_) => TargetProfile::default(),
        None => store
            .machine_profile(this)
            .map_err(err)?
            .unwrap_or_default(),
    };
    let mut target = base.clone();
    target.rules.extend(store.ignore_rules(this).map_err(err)?);
    Ok((base, target))
}

type Fetched = (String, Vec<u8>);
