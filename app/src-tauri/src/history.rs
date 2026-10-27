//! History: this machine's scans and every file change Patch Pilot made,
//! newest first, with Undo and "go back to this scan" (decision #45).
//! Plug-ins are only listed; files are put back through
//! `patchpilot_local::restore`.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use patchpilot_core::{ItemKind, Manifest};
use patchpilot_local::restore::{self, ApplyError, UndoFile};
use patchpilot_parity::{ProductChange, plugin_changes};
use patchpilot_store::{RestoreKind, Store};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::rig::{db_path, err, open};

/// Scans further back than this aren't listed (each one is a manifest load).
const MAX_SCANS: usize = 60;

/// What a scan changed since the one before and how many plug-in products
/// it had, by the two scans' manifest hashes (the same contents always give
/// the same answer, whichever database they're in).
type ScanKey = (String, Option<String>);
static SCAN_CHANGES: Mutex<BTreeMap<ScanKey, (Vec<ProductChange>, usize)>> =
    Mutex::new(BTreeMap::new());

/// Mirrored by `HistoryView` in `src/api.ts`.
#[derive(Debug, Serialize)]
pub struct HistoryView {
    pub entries: Vec<Entry>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Entry {
    /// A saved scan whose plug-ins differ from the one before (or the first).
    Scan {
        snapshot_id: i64,
        at: i64,
        first: bool,
        /// The newest scan: going back to it changes nothing.
        latest: bool,
        plugins: usize,
        changes: Vec<ProductChange>,
    },
    /// Files Patch Pilot changed, or put back.
    Files {
        id: i64,
        at: i64,
        label: String,
        undo: bool,
        undone: bool,
        files: Vec<FileLine>,
    },
}

#[derive(Debug, Serialize)]
pub struct FileLine {
    pub name: String,
    pub folder: String,
    pub path: String,
}

#[derive(Debug, Serialize)]
pub struct FilePreview {
    #[serde(flatten)]
    pub file: FileLine,
    /// Put back (true) or removed because the change created it (false).
    pub restore: bool,
    pub changed_since: bool,
    pub missing_backup: bool,
}

/// What going back to a scan would do.
#[derive(Debug, Serialize)]
pub struct GoBackPlan {
    pub at: i64,
    pub files: Vec<FilePreview>,
    /// From now to then: `added` = reinstall, `removed` = uninstall,
    /// `downgraded` / `updated` = install that version.
    pub plugins: Vec<ProductChange>,
}

#[tauri::command]
pub async fn history() -> Result<HistoryView, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let Some((store, id, _)) = context()? else {
            return Ok(HistoryView {
                entries: Vec::new(),
            });
        };
        history_view(&store, &id)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn undo_preview(id: i64) -> Result<Vec<FilePreview>, String> {
    let (store, _, blobs) = context()?.ok_or(NOT_SCANNED)?;
    let point = store
        .restore_point(id)
        .map_err(err)?
        .ok_or("that change is no longer in History")?;
    Ok(previews(restore::undo_preview(&blobs, &point)))
}

#[tauri::command]
pub async fn undo_change(app: AppHandle, id: i64) -> Result<HistoryView, String> {
    let (mut store, machine, blobs) = context()?.ok_or(NOT_SCANNED)?;
    let label = store.restore_point(id).map_err(err)?.map(|p| p.label);
    restore::undo(&mut store, &blobs, id, patchpilot_local::now()).map_err(apply_err)?;
    // Undoing a move of this Mac's shortcuts turns the setting the other way.
    if let (Some(label), Ok(dir)) = (label, crate::sync::app_dir()) {
        patchpilot_local::mac_keys::after_undo(&dir, &label)?;
    }
    let _ = app.emit("data-changed", ());
    history_view(&store, &machine)
}

#[tauri::command]
pub async fn go_back_plan(snapshot_id: i64) -> Result<GoBackPlan, String> {
    let (store, machine, blobs) = context()?.ok_or(NOT_SCANNED)?;
    go_back_plan_for(&store, &blobs, &machine, snapshot_id)
}

#[tauri::command]
pub async fn go_back(app: AppHandle, snapshot_id: i64) -> Result<HistoryView, String> {
    let (mut store, machine, blobs) = context()?.ok_or(NOT_SCANNED)?;
    let at = scan_time(&store, &machine, snapshot_id)?;
    let label = format!("Went back to the scan of {}", day(at));
    restore::go_back(
        &mut store,
        &blobs,
        &machine,
        snapshot_id,
        &label,
        patchpilot_local::now(),
    )
    .map_err(apply_err)?;
    let _ = app.emit("data-changed", ());
    history_view(&store, &machine)
}

const NOT_SCANNED: &str = "scan this machine first";

/// The store, this machine's ID and the blob folder; `None` before the first
/// scan.
fn context() -> Result<Option<(Store, String, PathBuf)>, String> {
    let db = db_path()?;
    if !db.exists() {
        return Ok(None);
    }
    let dir = patchpilot_local::app_dir(&db);
    let Some(id) = patchpilot_store::read_machine_id(dir).map_err(|e| e.to_string())? else {
        return Ok(None);
    };
    Ok(Some((open(&db)?, id, dir.join("blobs"))))
}

pub(crate) fn history_view(store: &Store, machine: &str) -> Result<HistoryView, String> {
    let mut entries = Vec::new();
    let snapshots = store.snapshot_times(machine).map_err(err)?;
    let latest = snapshots.first().map(|s| s.id);
    // Scans never change, so what each one changed is worked out once per
    // run; reopening History only reads scans saved since (2026-10-27).
    let mut cache = SCAN_CHANGES.lock().unwrap_or_else(|e| e.into_inner());
    let mut loaded: HashMap<i64, Manifest> = HashMap::new();
    for (i, scan) in snapshots.iter().take(MAX_SCANS).enumerate() {
        let id = scan.id;
        // The scan before this one; none for the first scan.
        let earlier = snapshots.get(i + 1);
        let before = earlier.map(|s| s.id);
        let key = (
            scan.manifest_hash.clone(),
            earlier.map(|s| s.manifest_hash.clone()),
        );
        if !cache.contains_key(&key) {
            load(store, &mut loaded, id)?;
            let plugins = patchpilot_parity::inventory(&loaded[&id]).products;
            let changes = match before {
                Some(b) => {
                    load(store, &mut loaded, b)?;
                    plugin_changes(&loaded[&b], &loaded[&id])
                }
                None => Vec::new(),
            };
            // Only the next-older scan is needed again.
            loaded.retain(|k, _| Some(*k) == before);
            cache.insert(key.clone(), (changes, plugins));
        }
        let (changes, plugins) = &cache[&key];
        let first = before.is_none();
        if changes.is_empty() && !first {
            continue;
        }
        entries.push(Entry::Scan {
            snapshot_id: id,
            at: scan.created_at,
            first,
            latest: Some(id) == latest,
            plugins: *plugins,
            changes: changes.clone(),
        });
    }
    for p in store.restore_points(machine).map_err(err)? {
        entries.push(Entry::Files {
            id: p.id,
            at: p.created_at,
            label: p.label,
            undo: p.kind == RestoreKind::Undo,
            undone: p.undone_by.is_some(),
            files: p.files.iter().map(|f| file_line(&f.path)).collect(),
        });
    }
    // Newest first; a file change made right after a scan sorts above it.
    entries.sort_by_key(|e| {
        std::cmp::Reverse(match e {
            Entry::Scan { at, .. } => (*at, 0),
            Entry::Files { at, .. } => (*at, 1),
        })
    });
    Ok(HistoryView { entries })
}

pub(crate) fn go_back_plan_for(
    store: &Store,
    blobs: &Path,
    machine: &str,
    snapshot_id: i64,
) -> Result<GoBackPlan, String> {
    let at = scan_time(store, machine, snapshot_id)?;
    let then = plugins_at(store, snapshot_id)?;
    let (latest, _) = store
        .latest_snapshot(machine)
        .map_err(err)?
        .ok_or(NOT_SCANNED)?;
    let now = plugins_at(store, latest)?;
    let files = restore::go_back_preview(store, blobs, machine, snapshot_id).map_err(apply_err)?;
    Ok(GoBackPlan {
        at,
        files: previews(files),
        plugins: plugin_changes(&now, &then),
    })
}

fn scan_time(store: &Store, machine: &str, snapshot_id: i64) -> Result<i64, String> {
    store
        .snapshot_times(machine)
        .map_err(err)?
        .iter()
        .find(|s| s.id == snapshot_id)
        .map(|s| s.created_at)
        .ok_or_else(|| "that scan isn't one of this machine's".to_owned())
}

/// A scan's plug-ins: all History compares, and far less to read than a
/// whole scan, which on a big rig is mostly presets and files.
fn plugins_at(store: &Store, snapshot_id: i64) -> Result<Manifest, String> {
    store
        .snapshot_manifest_of(snapshot_id, &[ItemKind::Plugin])
        .map_err(err)
}

fn load(store: &Store, loaded: &mut HashMap<i64, Manifest>, id: i64) -> Result<(), String> {
    if let std::collections::hash_map::Entry::Vacant(e) = loaded.entry(id) {
        e.insert(plugins_at(store, id)?);
    }
    Ok(())
}

fn previews(files: Vec<UndoFile>) -> Vec<FilePreview> {
    files
        .into_iter()
        .map(|f| FilePreview {
            file: file_line(&f.path.to_string_lossy()),
            restore: f.restore,
            changed_since: f.changed_since,
            missing_backup: f.missing_backup,
        })
        .collect()
}

fn file_line(path: &str) -> FileLine {
    // Split on either separator: History may show a path saved on this OS
    // by an older build, and tests run on every OS.
    let (folder, name) = path.rsplit_once(['/', '\\']).unwrap_or(("", path));
    let folder = folder.rsplit(['/', '\\']).take(2).collect::<Vec<_>>();
    FileLine {
        name: name.to_owned(),
        folder: folder.into_iter().rev().collect::<Vec<_>>().join("/"),
        path: path.to_owned(),
    }
}

fn apply_err(e: ApplyError) -> String {
    e.to_string()
}

/// "24 Oct 2026" style, from Unix seconds (UTC; the UI shows local times).
fn day(at: i64) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    // Civil-from-days (Howard Hinnant).
    let z = at.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{d} {} {y}", MONTHS[(m - 1) as usize])
}

#[cfg(test)]
mod tests {
    use super::*;
    use patchpilot_local::restore::{ChangeSet, FileChange, apply_changes};
    use patchpilot_store::SnapshotReason;

    fn fixture() -> Manifest {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/core/manifest-v1/studio-pc.json");
        Manifest::from_json(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn history_lists_scans_with_changes_and_file_changes() {
        let dir = tempfile::tempdir().unwrap();
        let blobs = dir.path().join("blobs");
        let mut store = Store::open_in_memory().unwrap();
        let m = fixture();
        let id = m.machine.id.clone();
        let first = store
            .save_manifest(&m, SnapshotReason::FirstScan, 100)
            .unwrap()
            .id;

        let file = dir.path().join("TrackTemplates").join("Vox.RTrackTemplate");
        apply_changes(
            &mut store,
            &blobs,
            &ChangeSet {
                machine_id: id.clone(),
                label: "Brought over 1 Reaper file".into(),
                daw: None,
                changes: vec![FileChange {
                    path: file.clone(),
                    contents: Some(b"vox".to_vec()),
                }],
                now: 200,
            },
        )
        .unwrap();

        // Same plug-ins, new name: a scan that isn't listed.
        let mut same = m.clone();
        same.machine.name = "Renamed".into();
        same.generated_at += 1;
        store
            .save_manifest(&same, SnapshotReason::Scan, 250)
            .unwrap();
        // One plug-in gone.
        let mut fewer = same.clone();
        let gone = fewer
            .items
            .iter()
            .find(|i| i.kind == patchpilot_core::ItemKind::Plugin)
            .and_then(|i| i.product_id.clone())
            .unwrap();
        fewer.items.retain(|i| i.product_id.as_ref() != Some(&gone));
        fewer.generated_at += 1;
        store
            .save_manifest(&fewer, SnapshotReason::Scan, 300)
            .unwrap();

        let view = history_view(&store, &id).unwrap();
        let kinds: Vec<_> = view
            .entries
            .iter()
            .map(|e| match e {
                Entry::Scan {
                    first,
                    latest,
                    changes,
                    ..
                } => {
                    format!(
                        "scan first={first} latest={latest} changes={}",
                        changes.len()
                    )
                }
                Entry::Files { label, files, .. } => format!("{label} {}", files[0].name),
            })
            .collect();
        assert_eq!(
            kinds,
            [
                "scan first=false latest=true changes=1",
                "Brought over 1 Reaper file Vox.RTrackTemplate",
                "scan first=true latest=false changes=0",
            ]
        );

        let plan = go_back_plan_for(&store, &blobs, &id, first).unwrap();
        assert_eq!(plan.files.len(), 1);
        assert!(!plan.files[0].restore, "the change created it, so it goes");
        assert_eq!(plan.plugins.len(), 1);
        assert_eq!(plan.plugins[0].change, patchpilot_parity::Change::Added);
        assert_eq!(plan.plugins[0].product_id, gone.0);
    }

    #[test]
    fn short_folder_and_day() {
        let f = file_line(
            "/Users/a/Library/Application Support/REAPER/TrackTemplates/Vox.RTrackTemplate",
        );
        assert_eq!(f.name, "Vox.RTrackTemplate");
        assert_eq!(f.folder, "REAPER/TrackTemplates");
        let w = file_line(r"C:\Users\a\AppData\Roaming\REAPER\FXChains\Bus.RfxChain");
        assert_eq!(w.folder, "REAPER/FXChains");
        assert_eq!(day(1_793_000_000), "26 Oct 2026");
    }
}
