//! Keep files in sync (decision #70): new and changed scripts, templates,
//! FX chains, themes and presets travel between linked machines through the
//! backup folder (decision #67) with no clicks. Each machine backs up after
//! every change it scans; every other machine reads those backups and places
//! what's new or changed there through restore points, so History can undo
//! each placement.
//!
//! It is safe by design, because it changes files the user didn't touch:
//!
//! - Only files: settings (Reaper key maps and menus, Ableton's preferences)
//!   and the Installer Vault's index stay with Bring over and Restore.
//! - Never deletes: a file removed on one machine stays on the others, and a
//!   file deleted here isn't put back unless another machine changes it.
//! - Never replaces an edit: a file here is replaced only when it is still
//!   the copy last synced and the other machine's copy changed since it was
//!   last seen. Otherwise the two copies are a conflict the user settles
//!   (theirs or this one); both stay where they are meanwhile (the other in
//!   its machine's backup folder), with no conflicted-copy files.
//! - Respects Ignore (decision #65) and waits while Ableton Live is open
//!   (CLAUDE.md rule 5); waiting files are tried again later.
//! - Only from linked machines: a retired machine's old backup in the same
//!   folder can't bring stale files back.
//!
//! What was seen and synced is kept in app-data `autosync.json`.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use patchpilot_core::ContentHash;
use patchpilot_parity::ableton::{ABLETON_PREFS, ABLETON_USER_LIBRARY};
use patchpilot_parity::reaper::{REAPER_RESOURCES, resource_kind};
use patchpilot_store::Store;
use serde::{Deserialize, Serialize};

use crate::backup::{self, BackedUpFile, Backup, FORMAT_VERSION, VAULT_ROOT};
use crate::bring::{self, FileToBring};
use crate::restore::{Daw, daw_running};

const STATE_FILE: &str = "autosync.json";

/// What this machine has seen and synced. Keys are `root/rel`, lowercased
/// (NTFS and APFS compare paths without case).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncState {
    /// Per linked machine, the copy of each file last seen in its backup
    /// (placed here, already the same here, or settled in a conflict).
    #[serde(default)]
    pub seen: BTreeMap<String, BTreeMap<String, ContentHash>>,
    /// The copy each file had here after it was last synced: a file that
    /// still has it wasn't edited here since, so a newer copy may replace it.
    #[serde(default)]
    pub synced: BTreeMap<String, ContentHash>,
    /// Files changed both here and on another machine, waiting for the user.
    #[serde(default)]
    pub conflicts: Vec<Conflict>,
}

/// A file changed here and on another machine since they were last in sync.
/// Mirrored in `app/src/api.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conflict {
    pub app: String,
    pub kind: String,
    pub root: String,
    pub rel: String,
    pub machine_id: String,
    pub machine_name: String,
    /// The other machine's copy.
    pub theirs: ContentHash,
    /// Its backup's time, Unix seconds.
    pub changed_at: i64,
}

impl Conflict {
    fn key(&self) -> String {
        format!("{}/{}", self.root, self.rel).to_lowercase()
    }
}

pub fn load_state(app_dir: &Path) -> SyncState {
    std::fs::read(app_dir.join(STATE_FILE))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

pub fn save_state(app_dir: &Path, state: &SyncState) -> Result<(), String> {
    std::fs::create_dir_all(app_dir).map_err(|e| e.to_string())?;
    let json = serde_json::to_vec_pretty(state).expect("state serializes");
    let tmp = app_dir.join(format!("{STATE_FILE}.tmp"));
    std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, app_dir.join(STATE_FILE)).map_err(|e| e.to_string())
}

/// Files placed from one machine. Mirrored in `app/src/api.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Placed {
    pub machine_name: String,
    pub app: String,
    pub files: usize,
    /// New here, rather than a newer copy of a file here.
    pub new: usize,
    pub restore_point: Option<i64>,
}

/// What [`sync_in`] did. Mirrored in `app/src/api.ts`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct SyncReport {
    pub placed: Vec<Placed>,
    /// Conflicts found this time (all of them are in the state).
    pub new_conflicts: usize,
    /// Why files are waiting, e.g. "Ableton Live is open": tried again later.
    pub waiting: Vec<String>,
    /// Files whose copy the cloud app hasn't downloaded yet.
    pub downloading: usize,
    /// What each machine's latest backup in the folder holds for this one,
    /// so the app can say why nothing came over.
    pub sources: Vec<SourceSummary>,
    /// When it looked, Unix seconds.
    pub looked_at: i64,
}

/// One machine's latest backup, as this machine sees it. Mirrored in
/// `app/src/api.ts`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct SourceSummary {
    pub machine_id: String,
    pub machine_name: String,
    /// The backup's time, Unix seconds.
    pub made_at: i64,
    /// Files in it that sync (not settings).
    pub files: usize,
    /// Of those, already the same here.
    pub same: usize,
    /// Brought here this time.
    pub placed: usize,
    /// Waiting for the user to pick a copy.
    pub to_choose: usize,
    /// Not downloaded by the cloud app yet.
    pub downloading: usize,
    /// False for a machine not linked to this one: its backup is left alone.
    pub linked: bool,
}

impl SyncReport {
    pub fn files_placed(&self) -> usize {
        self.placed.iter().map(|p| p.files).sum()
    }
}

/// True for files that keep themselves in sync: everything Bring over
/// handles except settings, which stay a deliberate step.
pub fn syncs(root: &str, rel: &str) -> bool {
    match root {
        REAPER_RESOURCES => resource_kind(rel).is_some_and(|k| !k.needs_reaper_closed()),
        ABLETON_PREFS | VAULT_ROOT => false,
        _ => true,
    }
}

/// A file one linked machine's latest backup offers.
struct Offer<'a> {
    backup: &'a Backup,
    held: &'a BackedUpFile,
    file: FileToBring,
}

/// The copy of `rel` below `dir` here, hashed as backups hash it; `None`
/// when there's no file. Files that sync are kept as they are (only
/// settings have a shared view), so that's the file's own hash.
fn hash_here(dir: Option<&Path>, rel: &str) -> Option<ContentHash> {
    let path = rel.split('/').fold(dir?.to_owned(), |p, part| p.join(part));
    let bytes = std::fs::read(path).ok()?;
    Some(patchpilot_sync::content_hash(&bytes))
}

/// Place what's new or changed in linked machines' latest backups in `dir`
/// (`Patch Pilot Backup`) on this machine (`this`), as described above.
/// `state` is updated; the caller saves it.
pub fn sync_in(
    store: &mut Store,
    blobs: &Path,
    this: &str,
    dir: &Path,
    state: &mut SyncState,
    now: i64,
) -> Result<SyncReport, String> {
    let machine = store
        .latest_manifest(this)
        .map_err(|e| e.to_string())?
        .ok_or("Scan this machine first.")?;
    let linked: BTreeSet<String> = store
        .machines()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|m| m.id)
        .filter(|id| id != this)
        .collect();
    let profile = backup::restore_profile(store, this)?;
    // The machine the Primary Rig was last made from: the reference when two
    // machines first meet with different copies of a file.
    let primary = store
        .golden(None)
        .map_err(|e| e.to_string())?
        .and_then(|g| g.info.source_machine_id);
    let mut report = SyncReport {
        looked_at: now,
        ..SyncReport::default()
    };
    let mut backups: Vec<Backup> = Vec::new();
    for b in backup::subdirs(dir)
        .iter()
        .filter_map(|f| backup::latest(f))
    {
        if b.machine_id == this || b.format > FORMAT_VERSION {
            continue;
        }
        let ok = linked.contains(&b.machine_id);
        report.sources.push(SourceSummary {
            machine_id: b.machine_id.clone(),
            machine_name: b.machine_name.clone(),
            made_at: b.made_at,
            files: b.files.iter().filter(|f| syncs(&f.root, &f.rel)).count(),
            linked: ok,
            ..SourceSummary::default()
        });
        if ok {
            backups.push(b);
        }
    }
    // Machines unlinked since are forgotten.
    state.seen.retain(|id, _| linked.contains(id));
    state.conflicts.retain(|c| linked.contains(&c.machine_id));
    // Copies that differed before the machines ever synced were asked about
    // before 0.4.8; the Primary Rig's machine now decides them, so look again.
    if let Some(p) = &primary {
        let first_meet: Vec<Conflict> = state
            .conflicts
            .iter()
            .filter(|c| (&c.machine_id == p || p == this) && !state.synced.contains_key(&c.key()))
            .cloned()
            .collect();
        for c in first_meet {
            if let Some(seen) = state.seen.get_mut(&c.machine_id) {
                seen.remove(&c.key());
            }
            state
                .conflicts
                .retain(|x| !(x.key() == c.key() && x.machine_id == c.machine_id));
        }
    }

    let mut dirs: HashMap<String, Option<PathBuf>> = HashMap::new();
    let mut root_dir = |root: &str| {
        dirs.entry(root.to_owned())
            .or_insert_with(|| bring::root_dir(Some(&machine), root))
            .clone()
    };
    let unseen = |state: &SyncState, id: &str, key: &str, hash: &ContentHash| {
        state.seen.get(id).and_then(|s| s.get(key)) != Some(hash)
    };

    // Every file a backup has that this machine lacks or has another copy
    // of (by its last scan), by key.
    let mut offers: BTreeMap<String, Vec<Offer>> = BTreeMap::new();
    for b in &backups {
        let held: HashMap<String, &BackedUpFile> = b
            .files
            .iter()
            .map(|f| (f.key().to_lowercase(), f))
            .collect();
        let mut listed = BTreeSet::new();
        for mut file in bring::files_to_bring(&b.scan, &profile, Some(&machine)) {
            let key = file.key().to_lowercase();
            let Some(&held) = held.get(&key).filter(|_| syncs(&file.root, &file.rel)) else {
                continue;
            };
            listed.insert(key.clone());
            file.hash = held.hash.clone();
            offers.entry(key).or_default().push(Offer {
                backup: b,
                held,
                file,
            });
        }
        // The rest is the same here by the scan: confirm it on disk once per
        // copy, so a later change there is known to be one.
        for (key, f) in &held {
            if listed.contains(key)
                || !syncs(&f.root, &f.rel)
                || !unseen(state, &b.machine_id, key, &f.hash)
            {
                continue;
            }
            if hash_here(root_dir(&f.root).as_deref(), &f.rel).as_ref() == Some(&f.hash) {
                state
                    .seen
                    .entry(b.machine_id.clone())
                    .or_default()
                    .insert(key.clone(), f.hash.clone());
                state.synced.insert(key.clone(), f.hash.clone());
            }
        }
    }

    // What to write: (machine, root) to the files, and what each needs.
    let mut chosen: Vec<Offer> = Vec::new();
    for (key, list) in offers {
        let unseen: Vec<Offer> = list
            .into_iter()
            .filter(|o| unseen(state, &o.backup.machine_id, &key, &o.held.hash))
            .collect();
        if unseen.is_empty() {
            continue;
        }
        let first = &unseen[0].file;
        let here_dir = root_dir(&first.root);
        let here = hash_here(here_dir.as_deref(), &first.rel);
        let synced = state.synced.get(&key).cloned();
        // The newest backup's copy wins among machines that changed it.
        let newest = unseen
            .iter()
            .enumerate()
            .max_by_key(|(_, o)| o.backup.made_at)
            .map(|(i, _)| i)
            .unwrap_or(0);
        let mut take = None;
        for (i, o) in unseen.into_iter().enumerate() {
            let hash = &o.held.hash;
            let seen_before = state
                .seen
                .get(&o.backup.machine_id)
                .is_some_and(|s| s.contains_key(&key));
            let mark = |state: &mut SyncState| {
                state
                    .seen
                    .entry(o.backup.machine_id.clone())
                    .or_default()
                    .insert(key.clone(), hash.clone());
            };
            if here.as_ref() == Some(hash) {
                mark(state);
                state.synced.insert(key.clone(), hash.clone());
                state.conflicts.retain(|c| c.key() != key);
                continue;
            }
            if i != newest {
                // An older copy from another machine: the newest one decides.
                mark(state);
                continue;
            }
            let from_primary = primary.as_deref() == Some(o.backup.machine_id.as_str());
            let this_primary = primary.as_deref() == Some(this);
            let place = match (&here, &synced) {
                // New here, or deleted here and since changed there.
                (None, None) => true,
                (None, Some(_)) => seen_before,
                // Not edited here since the last sync, and changed there.
                (Some(h), Some(s)) => h == s && seen_before,
                // Different before the two machines ever synced it: the
                // Primary Rig's machine is the reference, so its copy comes
                // here and others' copies don't go to it.
                (Some(_), None) => from_primary,
            };
            let keep_here = matches!((&here, &synced), (Some(_), None)) && this_primary;
            if place {
                take = Some(o);
            } else if let (true, Some(h)) = (keep_here, &here) {
                mark(state);
                state.synced.insert(key.clone(), h.clone());
            } else if here.is_some() {
                // Edited both here and there (or never synced): ask.
                state
                    .conflicts
                    .retain(|c| !(c.key() == key && c.machine_id == o.backup.machine_id));
                state.conflicts.push(Conflict {
                    app: o.file.app.clone(),
                    kind: o.file.kind.clone(),
                    root: o.file.root.clone(),
                    rel: o.file.rel.clone(),
                    machine_id: o.backup.machine_id.clone(),
                    machine_name: o.backup.machine_name.clone(),
                    theirs: hash.clone(),
                    changed_at: o.backup.made_at,
                });
                report.new_conflicts += 1;
                mark(state);
            } else {
                // Deleted here: stays deleted.
                mark(state);
            }
        }
        chosen.extend(take);
    }

    // Read every chosen file; files needing one that isn't there yet wait.
    let list: Vec<FileToBring> = chosen.iter().map(|o| o.file.clone()).collect();
    let mut bytes: HashMap<String, Vec<u8>> = HashMap::new();
    let mut missing = BTreeSet::new();
    for o in &chosen {
        match backup::bytes_of(o.backup, o.held) {
            Some(b) => {
                bytes.insert(o.file.key(), b);
            }
            None => {
                missing.insert(o.file.key());
            }
        }
    }
    let held_back = bring::blocked(&list, &missing);
    report.downloading = missing.len() + held_back.len();
    for o in &chosen {
        let k = o.file.key();
        if missing.contains(&k) || held_back.contains(&k) {
            source(&mut report, &o.backup.machine_id).downloading += 1;
        }
    }

    let mut groups: BTreeMap<(String, String, String), Vec<&Offer>> = BTreeMap::new();
    for o in &chosen {
        let k = o.file.key();
        if missing.contains(&k) || held_back.contains(&k) {
            continue;
        }
        groups
            .entry((
                o.backup.machine_id.clone(),
                o.file.root.clone(),
                o.file.app.clone(),
            ))
            .or_default()
            .push(o);
    }
    let mut waiting = BTreeSet::new();
    for ((_, root, app), offers) in groups {
        let Some(here_dir) = root_dir(&root) else {
            waiting.insert(format!("{app}'s folder isn't on this machine yet"));
            continue;
        };
        if root == ABLETON_USER_LIBRARY && daw_running(Daw::Ableton) {
            waiting.insert(format!("{} is open", Daw::Ableton.display_name()));
            continue;
        }
        let from = &offers[0].backup;
        let new = offers
            .iter()
            .filter(|o| hash_here(Some(&here_dir), &o.file.rel).is_none())
            .count();
        let files: Vec<(String, Vec<u8>)> = offers
            .iter()
            .filter_map(|o| Some((o.file.rel.clone(), bytes.get(&o.file.key())?.clone())))
            .collect();
        let label = format!(
            "Kept in sync: {} {app} file{} from {}",
            files.len(),
            if files.len() == 1 { "" } else { "s" },
            from.machine_name
        );
        match bring::write_files(store, blobs, this, &label, &root, &here_dir, &files, now) {
            Ok(w) => {
                for o in &offers {
                    let key = o.file.key().to_lowercase();
                    state
                        .seen
                        .entry(o.backup.machine_id.clone())
                        .or_default()
                        .insert(key.clone(), o.held.hash.clone());
                    state.synced.insert(key.clone(), o.held.hash.clone());
                    state.conflicts.retain(|c| c.key() != key);
                }
                source(&mut report, &from.machine_id).placed += w.written;
                report.placed.push(Placed {
                    machine_name: from.machine_name.clone(),
                    app,
                    files: w.written,
                    new,
                    restore_point: w.restore_point,
                });
            }
            Err(e) => {
                waiting.insert(e);
            }
        }
    }
    report.placed.retain(|p| p.files > 0);
    for src in &mut report.sources {
        let Some(b) = backups.iter().find(|b| b.machine_id == src.machine_id) else {
            continue;
        };
        let seen = state.seen.get(&b.machine_id);
        src.same = b
            .files
            .iter()
            .filter(|f| syncs(&f.root, &f.rel))
            .filter(|f| {
                let key = f.key().to_lowercase();
                seen.and_then(|s| s.get(&key)) == Some(&f.hash)
                    && state.synced.get(&key) == Some(&f.hash)
            })
            .count();
        src.to_choose = state
            .conflicts
            .iter()
            .filter(|c| c.machine_id == b.machine_id)
            .count();
    }
    report.waiting = waiting.into_iter().collect();
    Ok(report)
}

fn source<'a>(report: &'a mut SyncReport, id: &str) -> &'a mut SourceSummary {
    let i = report
        .sources
        .iter()
        .position(|s| s.machine_id == id)
        .expect("every backup read has a summary");
    &mut report.sources[i]
}

/// Settle a conflict on `root/rel` with `machine_id`: take that machine's
/// copy (`theirs`, through a restore point) or keep this machine's. Either
/// way the file is in sync again from here on.
#[allow(clippy::too_many_arguments)]
pub fn settle(
    store: &mut Store,
    blobs: &Path,
    this: &str,
    dir: &Path,
    state: &mut SyncState,
    root: &str,
    rel: &str,
    machine_id: &str,
    theirs: bool,
    now: i64,
) -> Result<(), String> {
    let key = format!("{root}/{rel}").to_lowercase();
    let conflict = state
        .conflicts
        .iter()
        .find(|c| c.key() == key && c.machine_id == machine_id)
        .cloned()
        .ok_or("That file isn't waiting on a choice any more.")?;
    let machine = store.latest_manifest(this).map_err(|e| e.to_string())?;
    let here_dir = bring::root_dir(machine.as_ref(), root)
        .ok_or_else(|| format!("Can't find {}'s folder on this machine.", conflict.app))?;
    if theirs {
        let b = backup::subdirs(dir)
            .iter()
            .filter_map(|f| backup::latest(f))
            .find(|b| b.machine_id == machine_id)
            .ok_or_else(|| format!("Can't find {}'s backup.", conflict.machine_name))?;
        let held = b
            .files
            .iter()
            .find(|f| f.key().to_lowercase() == key && f.hash == conflict.theirs)
            .ok_or_else(|| {
                format!(
                    "{} has changed this file again; it will be asked about shortly.",
                    conflict.machine_name
                )
            })?;
        let bytes = backup::bytes_of(&b, held)
            .ok_or("That copy hasn't finished downloading yet. Try again shortly.")?;
        let label = format!(
            "Kept in sync: used {}'s copy of {}",
            conflict.machine_name,
            rel.rsplit('/').next().unwrap_or(rel)
        );
        bring::write_files(
            store,
            blobs,
            this,
            &label,
            root,
            &here_dir,
            &[(rel.to_owned(), bytes)],
            now,
        )?;
        state.synced.insert(key.clone(), conflict.theirs.clone());
    } else if let Some(h) = hash_here(Some(&here_dir), rel) {
        // This copy is now the synced one: the next backup sends it out.
        state.synced.insert(key.clone(), h);
    }
    state
        .seen
        .entry(machine_id.to_owned())
        .or_default()
        .insert(key.clone(), conflict.theirs);
    state
        .conflicts
        .retain(|c| !(c.key() == key && c.machine_id == machine_id));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::back_up;
    use patchpilot_core::{
        Arch, Confidence, Item, ItemKind, MachineInfo, Manifest, Os, PathMap, PortablePath,
    };
    use patchpilot_store::SnapshotReason;

    const DRUMS: &str = "TrackTemplates/Drums.RTrackTemplate";
    const VOCAL: &str = "FXChains/Vocal Bus.RfxChain";
    const KEYS: &str = "reaper-kb.ini";

    fn path(dir: &Path, rel: &str) -> PathBuf {
        rel.split('/').fold(dir.to_owned(), |p, part| p.join(part))
    }

    fn put(dir: &Path, rel: &str, body: &str) {
        let p = path(dir, rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }

    fn read(dir: &Path, rel: &str) -> Option<String> {
        std::fs::read_to_string(path(dir, rel)).ok()
    }

    /// Scan `id`'s Reaper folder `dir` as it is now, and save it.
    fn scan(store: &mut Store, id: &str, name: &str, dir: &Path, rels: &[&str], at: i64) {
        let mut m = Manifest::new(
            MachineInfo {
                id: id.into(),
                name: name.into(),
                os: Os::Macos,
                os_version: None,
                arch: Arch::Aarch64,
                path_map: PathMap::new(Os::Macos)
                    .with_root(REAPER_RESOURCES, dir.to_string_lossy()),
            },
            at,
        );
        m.items = rels
            .iter()
            .filter(|r| path(dir, r).is_file())
            .map(|r| Item {
                kind: ItemKind::DawAsset,
                product_id: None,
                format: None,
                platform: Os::Macos,
                arch: None,
                version: None,
                path: PortablePath::logical(REAPER_RESOURCES, *r),
                content_hash: None,
                size: std::fs::metadata(path(dir, r)).ok().map(|m| m.len()),
                mtime: None,
                platform_portable: true,
                confidence: Confidence::High,
                requires: Vec::new(),
                needs_files: Vec::new(),
            })
            .collect();
        store.save_manifest(&m, SnapshotReason::Scan, at).unwrap();
    }

    #[test]
    fn places_new_and_changed_files_and_asks_about_edits_on_both() {
        let tmp = tempfile::tempdir().unwrap();
        let mac = tmp.path().join("Mac REAPER");
        let pc = tmp.path().join("PC REAPER");
        let dir = tmp.path().join("Dropbox").join(backup::FOLDER_NAME);
        let blobs = tmp.path().join("blobs");
        let all = [DRUMS, VOCAL, KEYS];
        let mut store = Store::open_in_memory().unwrap();
        let mut state = SyncState::default();

        put(&mac, DRUMS, "drums v1");
        put(&mac, VOCAL, "vocal");
        put(&mac, KEYS, "[keys] mac");
        put(&pc, DRUMS, "drums v1");
        put(&pc, KEYS, "[keys] pc");
        scan(&mut store, "mac", "MacBook", &mac, &all, 1);
        scan(&mut store, "pc", "Studio PC", &pc, &all, 1);
        back_up(&store, "mac", None, &dir, 1_000).unwrap();

        // New file placed; the same one left; settings never synced.
        let r = sync_in(&mut store, &blobs, "pc", &dir, &mut state, 2).unwrap();
        assert_eq!(r.files_placed(), 1, "{r:?}");
        assert_eq!(r.placed[0].new, 1);
        assert_eq!(read(&pc, VOCAL).as_deref(), Some("vocal"));
        assert_eq!(read(&pc, KEYS).as_deref(), Some("[keys] pc"));
        assert_eq!(r.new_conflicts, 0);
        let points = store.restore_points("pc").unwrap();
        assert_eq!(points.len(), 1);
        assert_eq!(points[0].label, "Kept in sync: 1 Reaper file from MacBook");

        // Nothing new: nothing done.
        let again = sync_in(&mut store, &blobs, "pc", &dir, &mut state, 3).unwrap();
        assert_eq!((again.files_placed(), again.new_conflicts), (0, 0));
        let src = &again.sources[0];
        assert_eq!(
            (src.machine_name.as_str(), src.files, src.same, src.linked),
            ("MacBook", 2, 2, true),
            "settings aren't counted"
        );

        // Changed on the Mac: replaces the PC's untouched copy.
        put(&mac, DRUMS, "drums v2");
        scan(&mut store, "mac", "MacBook", &mac, &all, 4);
        back_up(&store, "mac", None, &dir, 2_000).unwrap();
        let r = sync_in(&mut store, &blobs, "pc", &dir, &mut state, 5).unwrap();
        assert_eq!((r.files_placed(), r.placed[0].new), (1, 0));
        assert_eq!(read(&pc, DRUMS).as_deref(), Some("drums v2"));

        // Edited on both: the PC's copy stays and the user is asked.
        put(&pc, DRUMS, "drums pc edit");
        put(&mac, DRUMS, "drums v3");
        scan(&mut store, "mac", "MacBook", &mac, &all, 6);
        back_up(&store, "mac", None, &dir, 3_000).unwrap();
        let r = sync_in(&mut store, &blobs, "pc", &dir, &mut state, 7).unwrap();
        assert_eq!((r.files_placed(), r.new_conflicts), (0, 1));
        assert_eq!(read(&pc, DRUMS).as_deref(), Some("drums pc edit"));
        assert_eq!(state.conflicts.len(), 1);
        assert_eq!(state.conflicts[0].machine_name, "MacBook");
        // Asked once, not on every look.
        let r = sync_in(&mut store, &blobs, "pc", &dir, &mut state, 8).unwrap();
        assert_eq!(r.new_conflicts, 0);
        assert_eq!(state.conflicts.len(), 1);

        // Take the Mac's: placed through a restore point, and settled.
        settle(
            &mut store,
            &blobs,
            "pc",
            &dir,
            &mut state,
            REAPER_RESOURCES,
            DRUMS,
            "mac",
            true,
            9,
        )
        .unwrap();
        assert_eq!(read(&pc, DRUMS).as_deref(), Some("drums v3"));
        assert!(state.conflicts.is_empty());
        assert_eq!(store.restore_points("pc").unwrap().len(), 3);

        // Deleted on the Mac: stays on the PC.
        std::fs::remove_file(path(&mac, VOCAL)).unwrap();
        scan(&mut store, "mac", "MacBook", &mac, &all, 10);
        back_up(&store, "mac", None, &dir, 4_000).unwrap();
        sync_in(&mut store, &blobs, "pc", &dir, &mut state, 11).unwrap();
        assert_eq!(read(&pc, VOCAL).as_deref(), Some("vocal"));
    }

    #[test]
    fn keeping_this_copy_sends_it_out_and_deleted_files_stay_deleted() {
        let tmp = tempfile::tempdir().unwrap();
        let mac = tmp.path().join("Mac REAPER");
        let pc = tmp.path().join("PC REAPER");
        let dir = tmp.path().join(backup::FOLDER_NAME);
        let blobs = tmp.path().join("blobs");
        let all = [DRUMS, VOCAL];
        let mut store = Store::open_in_memory().unwrap();
        let mut state = SyncState::default();

        // Different copies before syncing was on: a conflict, never a
        // silent replace.
        put(&mac, DRUMS, "mac drums");
        put(&mac, VOCAL, "vocal");
        put(&pc, DRUMS, "pc drums");
        scan(&mut store, "mac", "MacBook", &mac, &all, 1);
        scan(&mut store, "pc", "Studio PC", &pc, &all, 1);
        back_up(&store, "mac", None, &dir, 1_000).unwrap();
        let r = sync_in(&mut store, &blobs, "pc", &dir, &mut state, 2).unwrap();
        assert_eq!((r.files_placed(), r.new_conflicts), (1, 1));
        assert_eq!(read(&pc, DRUMS).as_deref(), Some("pc drums"));

        // Keep the PC's: the Mac's copy isn't asked about again.
        settle(
            &mut store,
            &blobs,
            "pc",
            &dir,
            &mut state,
            REAPER_RESOURCES,
            DRUMS,
            "mac",
            false,
            3,
        )
        .unwrap();
        let r = sync_in(&mut store, &blobs, "pc", &dir, &mut state, 4).unwrap();
        assert_eq!((r.files_placed(), r.new_conflicts), (0, 0));

        // The PC's backup goes out; the Mac, untouched since, takes it.
        scan(&mut store, "pc", "Studio PC", &pc, &all, 5);
        back_up(&store, "pc", None, &dir, 2_000).unwrap();
        let mut mac_state = SyncState::default();
        let r = sync_in(&mut store, &blobs, "mac", &dir, &mut mac_state, 6).unwrap();
        assert_eq!(r.new_conflicts, 1, "never synced on the Mac: asked");
        settle(
            &mut store,
            &blobs,
            "mac",
            &dir,
            &mut mac_state,
            REAPER_RESOURCES,
            DRUMS,
            "pc",
            true,
            7,
        )
        .unwrap();
        assert_eq!(read(&mac, DRUMS).as_deref(), Some("pc drums"));

        // Deleted on the PC: not put back while the Mac's copy is the same.
        std::fs::remove_file(path(&pc, VOCAL)).unwrap();
        let r = sync_in(&mut store, &blobs, "pc", &dir, &mut state, 8).unwrap();
        assert_eq!(r.files_placed(), 0);
        assert!(read(&pc, VOCAL).is_none());
        // ...until the Mac changes it.
        put(&mac, VOCAL, "vocal v2");
        scan(&mut store, "mac", "MacBook", &mac, &all, 9);
        back_up(&store, "mac", None, &dir, 3_000).unwrap();
        sync_in(&mut store, &blobs, "pc", &dir, &mut state, 10).unwrap();
        assert_eq!(read(&pc, VOCAL).as_deref(), Some("vocal v2"));
    }

    #[test]
    fn ignored_files_stay_put() {
        let tmp = tempfile::tempdir().unwrap();
        let mac = tmp.path().join("Mac REAPER");
        let pc = tmp.path().join("PC REAPER");
        let dir = tmp.path().join(backup::FOLDER_NAME);
        let mut store = Store::open_in_memory().unwrap();
        put(&mac, VOCAL, "vocal");
        std::fs::create_dir_all(&pc).unwrap();
        scan(&mut store, "mac", "MacBook", &mac, &[VOCAL], 1);
        scan(&mut store, "pc", "Studio PC", &pc, &[VOCAL], 1);
        back_up(&store, "mac", None, &dir, 1_000).unwrap();
        store
            .set_ignore(&patchpilot_core::IgnoreEntry {
                machine_id: Some("pc".into()),
                label: "Vocal Bus".into(),
                rules: vec![patchpilot_core::Matcher::PathGlob(format!(
                    "{REAPER_RESOURCES}/{VOCAL}"
                ))],
                at: 1,
                undone: false,
            })
            .unwrap();
        let mut state = SyncState::default();
        let r = sync_in(&mut store, &tmp.path().join("b"), "pc", &dir, &mut state, 2).unwrap();
        assert_eq!(r.files_placed(), 0);
        assert!(read(&pc, VOCAL).is_none());
    }

    #[test]
    fn the_primary_rigs_machine_is_the_reference_when_machines_first_meet() {
        let tmp = tempfile::tempdir().unwrap();
        let pc = tmp.path().join("PC REAPER");
        let mac = tmp.path().join("Mac REAPER");
        let dir = tmp.path().join(backup::FOLDER_NAME);
        let blobs = tmp.path().join("blobs");
        let mut store = Store::open_in_memory().unwrap();
        // The PC is the Primary Rig; the MacBook has an older copy.
        put(&pc, DRUMS, "drums new");
        put(&mac, DRUMS, "drums old");
        put(&mac, VOCAL, "mac only");
        scan(&mut store, "pc", "Studio PC", &pc, &[DRUMS, VOCAL], 1);
        scan(&mut store, "mac", "MacBook", &mac, &[DRUMS, VOCAL], 1);
        let primary = store.latest_manifest("pc").unwrap().unwrap();
        store.save_golden(&primary, Some("pc"), None, 2).unwrap();
        back_up(&store, "pc", None, &dir, 1_000).unwrap();
        back_up(&store, "mac", None, &dir, 1_000).unwrap();

        // The MacBook takes the PC's copy without asking.
        let mut mac_state = SyncState::default();
        let r = sync_in(&mut store, &blobs, "mac", &dir, &mut mac_state, 3).unwrap();
        assert_eq!((r.files_placed(), r.new_conflicts), (1, 0), "{r:?}");
        assert_eq!(read(&mac, DRUMS).as_deref(), Some("drums new"));

        // The PC keeps its own copy, and takes the MacBook's new file.
        let mut pc_state = SyncState::default();
        let r = sync_in(&mut store, &blobs, "pc", &dir, &mut pc_state, 4).unwrap();
        assert_eq!((r.files_placed(), r.new_conflicts), (1, 0), "{r:?}");
        assert_eq!(read(&pc, DRUMS).as_deref(), Some("drums new"));
        assert_eq!(read(&pc, VOCAL).as_deref(), Some("mac only"));
    }

    #[test]
    fn first_meeting_conflicts_from_before_are_settled_by_the_primary_rig() {
        let tmp = tempfile::tempdir().unwrap();
        let pc = tmp.path().join("PC REAPER");
        let mac = tmp.path().join("Mac REAPER");
        let dir = tmp.path().join(backup::FOLDER_NAME);
        let blobs = tmp.path().join("blobs");
        let mut store = Store::open_in_memory().unwrap();
        put(&pc, DRUMS, "drums new");
        put(&mac, DRUMS, "drums old");
        scan(&mut store, "pc", "Studio PC", &pc, &[DRUMS], 1);
        scan(&mut store, "mac", "MacBook", &mac, &[DRUMS], 1);
        back_up(&store, "pc", None, &dir, 1_000).unwrap();
        // No Primary Rig yet: asked, as 0.4.6 did.
        let mut state = SyncState::default();
        let r = sync_in(&mut store, &blobs, "mac", &dir, &mut state, 2).unwrap();
        assert_eq!(r.new_conflicts, 1);
        // Once the PC is the Primary Rig, its copy comes over.
        let primary = store.latest_manifest("pc").unwrap().unwrap();
        store.save_golden(&primary, Some("pc"), None, 3).unwrap();
        let r = sync_in(&mut store, &blobs, "mac", &dir, &mut state, 4).unwrap();
        assert_eq!((r.files_placed(), state.conflicts.len()), (1, 0), "{r:?}");
        assert_eq!(read(&mac, DRUMS).as_deref(), Some("drums new"));
    }
}
