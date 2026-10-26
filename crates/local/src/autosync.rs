//! Keep files in sync (decision #70): new and changed scripts, templates,
//! FX chains, themes and presets travel between linked machines through the
//! backup folder (decision #67) with no clicks. Each machine backs up after
//! every change it scans; every other machine reads those backups and places
//! what's new or changed there through restore points, so History can undo
//! each placement.
//!
//! It is safe by design, because it changes files the user didn't touch:
//!
//! - Settings that travel (decision #77): Reaper's key map, menus and
//!   toolbars, mouse modifiers and theme adjuster settings, and Live's
//!   Collection names and Places in cloud folders (decision #79; only those
//!   from `Library.cfg`) and `Options.txt`.
//!   They are written only while their DAW is closed, and wait meanwhile.
//!   Machine settings (audio device, MIDI ports, plug-in folders, logins)
//!   are never read. The Installer Vault's index stays with Restore.
//! - Never deletes, except a rename: a file removed on one machine stays on
//!   the others, and a file deleted here isn't put back unless another
//!   machine changes it. A file renamed or moved on another machine (its
//!   copy gone from one path and the same copy at a new one) is renamed here
//!   too, only while this machine's copy is still that same copy, and in
//!   the same restore point as the new name, so History can undo it.
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

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use patchpilot_core::{ContentHash, Os};
use patchpilot_store::Store;
use serde::{Deserialize, Serialize};

use patchpilot_parity::ableton::{
    ABLETON_PREFS, LIBRARY_CFG, is_names_only, library_cfg_parts, places_not_here,
};

use crate::backup::{self, BackedUpFile, Backup, FORMAT_VERSION, VAULT_ROOT};
use crate::bring::{self, FileToBring};
use crate::restore::{Daw, daw_running};
use crate::sync_prefs::{self, SyncPrefs};

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
    /// What this state was written for; see [`STATE_VERSION`].
    #[serde(default)]
    pub version: u32,
}

/// 1: Live's `Library.cfg` shares Places too (decision #79), so what was
/// recorded for it before says nothing about the copies now: the two
/// machines meet again and the Primary Rig's copy wins.
const STATE_VERSION: u32 = 1;

/// The key of Live's `Library.cfg`.
fn library_cfg_key() -> String {
    format!("{ABLETON_PREFS}/{LIBRARY_CFG}").to_lowercase()
}

impl SyncState {
    fn upgrade(&mut self) {
        if self.version < 1 {
            let key = library_cfg_key();
            self.synced.remove(&key);
            for seen in self.seen.values_mut() {
                seen.remove(&key);
            }
            self.conflicts.retain(|c| c.key() != key);
        }
        self.version = STATE_VERSION;
    }
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
    /// Files renamed or moved here because they were there.
    pub renamed: usize,
    /// What the files written are ("Collection names", "Template"), each once.
    pub kinds: Vec<String>,
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
    /// Live Places on linked machines whose folder isn't on this one (yet),
    /// by name.
    pub missing_places: Vec<String>,
}

/// One machine's latest backup, as this machine sees it. Mirrored in
/// `app/src/api.ts`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct SourceSummary {
    pub machine_id: String,
    pub machine_name: String,
    /// The backup's time, Unix seconds.
    pub made_at: i64,
    /// Files in it that sync (every one but the Installer Vault's index).
    pub files: usize,
    /// Of those, already the same here.
    pub same: usize,
    /// Brought here this time.
    pub placed: usize,
    /// Renamed or moved here this time, as they were there.
    pub renamed: usize,
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
/// handles (only the portable settings are among them), not the Installer
/// Vault's index.
pub fn syncs(root: &str) -> bool {
    root != VAULT_ROOT
}

/// A file one linked machine's latest backup offers.
struct Offer<'a> {
    backup: &'a Backup,
    held: &'a BackedUpFile,
    file: FileToBring,
}

/// The copy of `rel` below `dir` (where `root` is) here, hashed as backups
/// hash it (a settings file by its shareable part); `None` when there's no
/// file.
fn hash_here(dir: Option<&Path>, root: &str, rel: &str) -> Option<ContentHash> {
    bring::shared_hash(dir?, root, rel)
}

/// How `rel` (lowercased) is spelled below `dir`, each part matched without
/// case as NTFS and APFS do; `None` when there's no such file.
fn spelled_here(dir: &Path, rel: &str) -> Option<String> {
    let mut path = dir.to_owned();
    let mut parts = Vec::new();
    for want in rel.split('/') {
        // Read the folder even where a lowercased path would open, so the
        // name is spelled as it is on disk.
        let name = std::fs::read_dir(&path)
            .ok()?
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .find(|n| n.to_lowercase() == want)?;
        path.push(&name);
        parts.push(name);
    }
    path.is_file().then(|| parts.join("/"))
}

/// A file a linked machine renamed or moved: its copy left `old` (a key,
/// lowercased) and turned up at `to`, a path new there.
struct Rename {
    machine_id: String,
    old: String,
    root: String,
    app: String,
    to: String,
    to_rel: String,
    hash: ContentHash,
}

/// The renames in `b`, a linked machine's latest backup, from what was seen
/// of it before. Keys it no longer has with no copy anywhere new were
/// deleted there; they're returned second.
fn renames_in(b: &Backup, state: &SyncState) -> (Vec<Rename>, Vec<String>) {
    let Some(seen) = state.seen.get(&b.machine_id) else {
        return (Vec::new(), Vec::new());
    };
    let held: HashSet<String> = b.files.iter().map(|f| f.key().to_lowercase()).collect();
    let mut new_at: HashMap<&ContentHash, Vec<&BackedUpFile>> = HashMap::new();
    for f in b.files.iter().filter(|f| syncs(&f.root)) {
        if !seen.contains_key(&f.key().to_lowercase()) {
            new_at.entry(&f.hash).or_default().push(f);
        }
    }
    let mut renames = Vec::new();
    let mut gone = Vec::new();
    for (old, hash) in seen {
        if held.contains(old) {
            continue;
        }
        let name = old.rsplit('/').next().unwrap_or(old);
        let candidates: Vec<&&BackedUpFile> = new_at
            .get(hash)
            .into_iter()
            .flatten()
            .filter(|f| old.starts_with(&format!("{}/", f.root.to_lowercase())))
            .collect();
        // Moved to another folder keeps its name; prefer that.
        let to = candidates
            .iter()
            .find(|f| f.rel.rsplit('/').next().unwrap_or(&f.rel).to_lowercase() == name)
            .or(candidates.first());
        match to {
            Some(f) => renames.push(Rename {
                machine_id: b.machine_id.clone(),
                old: old.clone(),
                root: f.root.clone(),
                app: f.app.clone(),
                to: f.key().to_lowercase(),
                to_rel: f.rel.clone(),
                hash: hash.clone(),
            }),
            None => gone.push(old.clone()),
        }
    }
    (renames, gone)
}

/// The OS paths here are spelled for (macOS's `/` where neither applies, as
/// in tests on Linux).
fn this_os() -> Os {
    Os::current().unwrap_or(Os::Macos)
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
    sync_in_with(store, blobs, this, dir, state, &SyncPrefs::default(), now)
}

/// [`sync_in`] with this machine's sync preferences (decision #80): kinds
/// turned off here, or on the machine a backup is from, are left alone.
pub fn sync_in_with(
    store: &mut Store,
    blobs: &Path,
    this: &str,
    dir: &Path,
    state: &mut SyncState,
    prefs: &SyncPrefs,
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
    state.upgrade();
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
            files: b.files.iter().filter(|f| syncs(&f.root)).count(),
            linked: ok,
            ..SourceSummary::default()
        });
        if ok {
            backups.push(only_what_syncs(b, prefs));
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

    // Renames and moves on linked machines. One whose old path here is gone
    // or edited is done with: an edit is never removed, and the new path
    // comes over like any new file. The rest wait for their new path here.
    let mut renames: Vec<Rename> = Vec::new();
    for b in &backups {
        let (found, gone) = renames_in(b, state);
        if let Some(seen) = state.seen.get_mut(&b.machine_id) {
            for key in gone {
                seen.remove(&key);
            }
        }
        for r in found {
            let old_rel = &r.old[r.root.len() + 1..];
            let here = root_dir(&r.root)
                .and_then(|d| Some((spelled_here(&d, old_rel)?, d)))
                .and_then(|(rel, d)| hash_here(Some(&d), &r.root, &rel));
            if here.as_ref() == Some(&r.hash) {
                renames.push(r);
            } else if let Some(seen) = state.seen.get_mut(&r.machine_id) {
                seen.remove(&r.old);
            }
        }
    }
    // A pending rename's new path isn't marked seen until the old one is
    // gone, so the rename is found again on the next look.
    let pending: HashSet<(String, String)> = renames
        .iter()
        .map(|r| (r.machine_id.clone(), r.to.clone()))
        .collect();

    // Every file a backup has that this machine lacks or has another copy
    // of (by its last scan), by key.
    let mut offers: BTreeMap<String, Vec<Offer>> = BTreeMap::new();
    let mut waiting = BTreeSet::new();
    let library_cfg = library_cfg_key();
    for b in &backups {
        let mut held: HashMap<String, &BackedUpFile> = b
            .files
            .iter()
            .map(|f| (f.key().to_lowercase(), f))
            .collect();
        // Collection names from a version before Places travelled would
        // leave this machine's Places looking edited: wait for its update.
        if let Some(f) = held.get(&library_cfg)
            && backup::bytes_of(b, f).is_some_and(|bytes| is_names_only(&bytes))
        {
            held.remove(&library_cfg);
            waiting.insert(format!(
                "Update Patch Pilot on {} to sync Live's Collection names and Places",
                b.machine_name
            ));
        }
        let mut listed = BTreeSet::new();
        for mut file in bring::files_to_bring(&b.scan, &profile, Some(&machine)) {
            let key = file.key().to_lowercase();
            let Some(&held) = held.get(&key).filter(|_| syncs(&file.root)) else {
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
                || !syncs(&f.root)
                || !unseen(state, &b.machine_id, key, &f.hash)
                || pending.contains(&(b.machine_id.clone(), key.clone()))
            {
                continue;
            }
            if hash_here(root_dir(&f.root).as_deref(), &f.root, &f.rel).as_ref() == Some(&f.hash) {
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
        let here = hash_here(here_dir.as_deref(), &first.root, &first.rel);
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
            // Live's Library.cfg shares only part of itself: a copy that
            // would change nothing here is in sync with it, whatever its hash.
            if key == library_cfg
                && let (Some(h), Some(d)) = (&here, &here_dir)
                && let Some(bytes) = shared_bytes(o.backup, o.held, prefs)
                && !bring::would_change(d, &o.file.root, &o.file.rel, bytes)
            {
                mark(state);
                state.synced.insert(key.clone(), h.clone());
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
        match shared_bytes(o.backup, o.held, prefs) {
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

    // One restore point per machine, root, app and DAW that must be closed,
    // so templates still come while Reaper is open and its key map waits.
    type GroupKey = (String, String, String, Option<&'static str>);
    let daw_of = |root: &str, rel: &str| bring::needs_closed(root, rel).map(Daw::as_str);
    let mut groups: BTreeMap<GroupKey, (Vec<&Offer>, Vec<&Rename>)> = BTreeMap::new();
    let mut coming: HashMap<String, (GroupKey, &ContentHash)> = HashMap::new();
    for o in &chosen {
        let k = o.file.key();
        if missing.contains(&k) || held_back.contains(&k) {
            continue;
        }
        let g = (
            o.backup.machine_id.clone(),
            o.file.root.clone(),
            o.file.app.clone(),
            daw_of(&o.file.root, &o.file.rel),
        );
        coming.insert(k.to_lowercase(), (g.clone(), &o.held.hash));
        groups.entry(g).or_default().0.push(o);
    }
    // Each rename goes with its new path's copy, or on its own when that
    // copy is here already; otherwise it waits for it.
    for r in &renames {
        let g = match coming.get(&r.to) {
            Some((g, hash)) if **hash == r.hash => g.clone(),
            Some(_) => continue,
            None => {
                let here = root_dir(&r.root);
                if hash_here(here.as_deref(), &r.root, &r.to_rel).as_ref() != Some(&r.hash) {
                    continue;
                }
                let old_rel = &r.old[r.root.len() + 1..];
                (
                    r.machine_id.clone(),
                    r.root.clone(),
                    r.app.clone(),
                    daw_of(&r.root, old_rel),
                )
            }
        };
        groups.entry(g).or_default().1.push(r);
    }
    for ((machine_id, root, app, daw), (offers, moves)) in groups {
        let Some(here_dir) = root_dir(&root) else {
            waiting.insert(format!("{app}'s folder isn't on this machine yet"));
            continue;
        };
        if let Some(d) = daw.and_then(Daw::parse)
            && daw_running(d)
        {
            waiting.insert(format!("{} is open", d.display_name()));
            continue;
        }
        let Some(from) = backups.iter().find(|b| b.machine_id == machine_id) else {
            continue;
        };
        let remove: Vec<String> = moves
            .iter()
            .filter_map(|r| spelled_here(&here_dir, &r.old[r.root.len() + 1..]))
            .collect();
        let new = offers
            .iter()
            .filter(|o| hash_here(Some(&here_dir), &root, &o.file.rel).is_none())
            .filter(|o| !moves.iter().any(|r| r.to == o.file.key().to_lowercase()))
            .count();
        let files: Vec<(String, Vec<u8>)> = offers
            .iter()
            .filter_map(|o| {
                let bytes = bytes.get(&o.file.key())?.clone();
                // Key maps arrive spelled portably: point them here.
                let bytes = bring::adapt_for_here(
                    &o.backup.scan,
                    &root,
                    &o.file.rel,
                    &here_dir,
                    this_os(),
                    bytes,
                );
                Some((o.file.rel.clone(), bytes))
            })
            .collect();
        let name = |rel: &str| rel.rsplit('/').next().unwrap_or(rel).to_owned();
        let only_the_move = |r: &Rename| match &files[..] {
            [] => true,
            [(rel, _)] => rel.to_lowercase() == r.to_rel.to_lowercase(),
            _ => false,
        };
        let label = match &moves[..] {
            [r] if only_the_move(r) => format!(
                "Kept in sync: {} renamed to {}, as on {}",
                name(&remove.first().cloned().unwrap_or_default()),
                name(&r.to_rel),
                from.machine_name
            ),
            _ => format!(
                "Kept in sync: {} {app} file{} from {}{}",
                files.len(),
                if files.len() == 1 { "" } else { "s" },
                from.machine_name,
                if moves.is_empty() {
                    String::new()
                } else {
                    format!(", {} renamed", moves.len())
                }
            ),
        };
        match bring::write_changes(
            store, blobs, this, &label, &root, &here_dir, &files, &remove, now,
        ) {
            Ok(w) => {
                for o in &offers {
                    let key = o.file.key().to_lowercase();
                    state
                        .seen
                        .entry(o.backup.machine_id.clone())
                        .or_default()
                        .insert(key.clone(), o.held.hash.clone());
                    // What's here now: a settings file merged into this
                    // machine's own may not hash as the other's copy.
                    let now_here = hash_here(Some(&here_dir), &root, &o.file.rel);
                    state
                        .synced
                        .insert(key.clone(), now_here.unwrap_or_else(|| o.held.hash.clone()));
                    state.conflicts.retain(|c| c.key() != key);
                }
                for r in &moves {
                    let seen = state.seen.entry(r.machine_id.clone()).or_default();
                    seen.remove(&r.old);
                    seen.insert(r.to.clone(), r.hash.clone());
                    state.synced.remove(&r.old);
                    state.synced.insert(r.to.clone(), r.hash.clone());
                    state.conflicts.retain(|c| c.key() != r.old);
                }
                let src = source(&mut report, &from.machine_id);
                src.placed += w.written;
                src.renamed += w.removed;
                let mut kinds: Vec<String> = offers.iter().map(|o| o.file.kind.clone()).collect();
                kinds.sort();
                kinds.dedup();
                report.placed.push(Placed {
                    machine_name: from.machine_name.clone(),
                    app,
                    files: w.written,
                    new,
                    renamed: w.removed,
                    kinds,
                    restore_point: w.restore_point,
                });
            }
            Err(e) => {
                waiting.insert(e);
            }
        }
    }
    report.placed.retain(|p| p.files > 0 || p.renamed > 0);
    for src in &mut report.sources {
        let Some(b) = backups.iter().find(|b| b.machine_id == src.machine_id) else {
            continue;
        };
        let seen = state.seen.get(&b.machine_id);
        src.same = b
            .files
            .iter()
            .filter(|f| syncs(&f.root))
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
    report.missing_places = missing_places(&backups, root_dir(ABLETON_PREFS).as_deref(), prefs);
    report.waiting = waiting.into_iter().collect();
    Ok(report)
}

/// Whether `id` syncs between this machine and `b`'s.
fn both_sync(b: &Backup, prefs: &SyncPrefs, id: &str) -> bool {
    prefs.syncs(id) && !b.not_synced.iter().any(|k| k == id)
}

/// `b` with only the files that sync between its machine and this one: a
/// kind turned off on either is left out, and so is `Library.cfg` when
/// both its parts are.
fn only_what_syncs(mut b: Backup, prefs: &SyncPrefs) -> Backup {
    let collections = both_sync(&b, prefs, sync_prefs::COLLECTIONS);
    let places = both_sync(&b, prefs, sync_prefs::PLACES);
    let key = library_cfg_key();
    let keep: Vec<bool> = b
        .files
        .iter()
        .map(|f| {
            if f.key().to_lowercase() == key {
                return collections || places;
            }
            sync_prefs::kind_of(&f.root, &f.rel).is_none_or(|id| both_sync(&b, prefs, id))
        })
        .collect();
    let mut keep = keep.into_iter();
    b.files.retain(|_| keep.next().unwrap_or(true));
    b
}

/// A backed-up file's bytes as they sync here: for `Library.cfg`, only the
/// parts both machines sync.
fn shared_bytes(b: &Backup, f: &BackedUpFile, prefs: &SyncPrefs) -> Option<Vec<u8>> {
    let bytes = backup::bytes_of(b, f)?;
    if f.key().to_lowercase() != library_cfg_key() {
        return Some(bytes);
    }
    let text = String::from_utf8_lossy(&bytes);
    Some(
        library_cfg_parts(
            &text,
            both_sync(b, prefs, sync_prefs::COLLECTIONS),
            both_sync(b, prefs, sync_prefs::PLACES),
        )
        .into_bytes(),
    )
}

/// The names of linked machines' Places whose folder isn't here: their
/// cloud folder isn't on this machine, or hasn't brought the folder yet.
fn missing_places(backups: &[Backup], dir: Option<&Path>, prefs: &SyncPrefs) -> Vec<String> {
    let Some(cfg) = dir.and_then(|d| std::fs::read_to_string(d.join(LIBRARY_CFG)).ok()) else {
        return Vec::new();
    };
    let clouds = crate::place_clouds();
    let key = library_cfg_key();
    let mut names = BTreeSet::new();
    for b in backups {
        let Some(f) = b.files.iter().find(|f| f.key().to_lowercase() == key) else {
            continue;
        };
        if let Some(bytes) = shared_bytes(b, f, prefs) {
            names.extend(places_not_here(
                &String::from_utf8_lossy(&bytes),
                &cfg,
                &clouds,
            ));
        }
    }
    names.into_iter().collect()
}

/// After files from `backup` (another machine's) were written here some
/// other way, such as Sync with: every file here that now matches the
/// backup's copy is in sync with it, so a later change there replaces it
/// like any synced file instead of being asked about as an edit here.
pub fn note_in_sync(
    store: &Store,
    this: &str,
    backup: &Backup,
    state: &mut SyncState,
) -> Result<(), String> {
    if backup.machine_id == this {
        return Ok(());
    }
    let machine = store.latest_manifest(this).map_err(|e| e.to_string())?;
    let mut dirs: HashMap<String, Option<PathBuf>> = HashMap::new();
    for f in backup.files.iter().filter(|f| syncs(&f.root)) {
        let dir = dirs
            .entry(f.root.clone())
            .or_insert_with(|| bring::root_dir(machine.as_ref(), &f.root))
            .clone();
        if hash_here(dir.as_deref(), &f.root, &f.rel).as_ref() != Some(&f.hash) {
            continue;
        }
        let key = f.key().to_lowercase();
        state
            .seen
            .entry(backup.machine_id.clone())
            .or_default()
            .insert(key.clone(), f.hash.clone());
        state.synced.insert(key.clone(), f.hash.clone());
        state.conflicts.retain(|c| c.key() != key);
    }
    Ok(())
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
        let bytes = bring::adapt_for_here(&b.scan, root, rel, &here_dir, this_os(), bytes);
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
    } else if let Some(h) = hash_here(Some(&here_dir), root, rel) {
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
    use patchpilot_parity::reaper::REAPER_RESOURCES;
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
        scan_roots(store, id, name, &[(REAPER_RESOURCES, dir, rels)], at);
    }

    /// Scan `id`'s folders (logical root, folder, files below it) as they
    /// are now, and save it.
    fn scan_roots(
        store: &mut Store,
        id: &str,
        name: &str,
        roots: &[(&str, &Path, &[&str])],
        at: i64,
    ) {
        let mut path_map = PathMap::new(Os::Macos);
        for (root, dir, _) in roots {
            path_map = path_map.with_root(*root, dir.to_string_lossy());
        }
        let mut m = Manifest::new(
            MachineInfo {
                id: id.into(),
                name: name.into(),
                os: Os::Macos,
                os_version: None,
                arch: Arch::Aarch64,
                path_map,
            },
            at,
        );
        for (root, dir, rels) in roots {
            m.items.extend(
                rels.iter()
                    .filter(|r| path(dir, r).is_file())
                    .map(|r| Item {
                        kind: ItemKind::DawAsset,
                        product_id: None,
                        format: None,
                        platform: Os::Macos,
                        arch: None,
                        version: None,
                        path: PortablePath::logical(*root, *r),
                        content_hash: None,
                        size: std::fs::metadata(path(dir, r)).ok().map(|m| m.len()),
                        mtime: None,
                        platform_portable: true,
                        confidence: Confidence::High,
                        requires: Vec::new(),
                        needs_files: Vec::new(),
                    }),
            );
        }
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
        put(&mac, KEYS, "[keys] same");
        put(&pc, DRUMS, "drums v1");
        put(&pc, KEYS, "[keys] same");
        scan(&mut store, "mac", "MacBook", &mac, &all, 1);
        scan(&mut store, "pc", "Studio PC", &pc, &all, 1);
        back_up(&store, "mac", None, &dir, 1_000).unwrap();

        // New file placed; the same ones left.
        let r = sync_in(&mut store, &blobs, "pc", &dir, &mut state, 2).unwrap();
        assert_eq!(r.files_placed(), 1, "{r:?}");
        assert_eq!(r.placed[0].new, 1);
        assert_eq!(r.placed[0].kinds, ["FX chain"]);
        assert_eq!(read(&pc, VOCAL).as_deref(), Some("vocal"));
        assert_eq!(read(&pc, KEYS).as_deref(), Some("[keys] same"));
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
            ("MacBook", 3, 3, true),
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
    fn kinds_turned_off_on_either_machine_stay_each_machines_own() {
        let tmp = tempfile::tempdir().unwrap();
        let mac = tmp.path().join("Mac REAPER");
        let pc = tmp.path().join("PC REAPER");
        let dir = tmp.path().join("Dropbox").join(backup::FOLDER_NAME);
        let blobs = tmp.path().join("blobs");
        let all = [DRUMS, VOCAL];
        let mut store = Store::open_in_memory().unwrap();
        let mut state = SyncState::default();
        put(&mac, DRUMS, "drums");
        put(&mac, VOCAL, "vocal");
        scan(&mut store, "mac", "MacBook", &mac, &all, 1);
        scan(&mut store, "pc", "Studio PC", &pc, &all, 1);
        back_up(&store, "mac", None, &dir, 1_000).unwrap();

        // Track templates off on the PC: only the FX chain comes.
        let mut prefs = SyncPrefs::default();
        prefs.set("reaper/track-templates", false);
        let r = sync_in_with(&mut store, &blobs, "pc", &dir, &mut state, &prefs, 2).unwrap();
        assert_eq!(r.files_placed(), 1, "{r:?}");
        assert_eq!(read(&pc, DRUMS), None);
        assert_eq!(read(&pc, VOCAL).as_deref(), Some("vocal"));

        // On again here, but the Mac keeps its track templates to itself.
        let off = vec!["reaper/track-templates".to_owned()];
        backup::back_up_with(&store, "mac", None, &dir, &off, 2_000).unwrap();
        let r = sync_in(&mut store, &blobs, "pc", &dir, &mut state, 3).unwrap();
        assert_eq!(r.files_placed(), 0, "{r:?}");
        assert_eq!(read(&pc, DRUMS), None);

        // On for both: it comes.
        backup::back_up_with(&store, "mac", None, &dir, &[], 3_000).unwrap();
        let r = sync_in(&mut store, &blobs, "pc", &dir, &mut state, 4).unwrap();
        assert_eq!(r.files_placed(), 1, "{r:?}");
        assert_eq!(read(&pc, DRUMS).as_deref(), Some("drums"));
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
    #[test]
    fn settings_travel_with_paths_for_here_and_wait_while_the_daw_is_open() {
        let tmp = tempfile::tempdir().unwrap();
        let mac = tmp.path().join("Mac REAPER");
        let pc = tmp.path().join("PC REAPER");
        let dir = tmp.path().join(backup::FOLDER_NAME);
        let blobs = tmp.path().join("blobs");
        const MOUSE: &str = "reaper-mouse.ini";
        let all = [KEYS, MOUSE, DRUMS];
        let mut store = Store::open_in_memory().unwrap();
        let mut state = SyncState::default();
        let script = |dir: &Path| path(dir, "Scripts/Tools/a.lua").display().to_string();

        let kb = |dir: &Path, key: u32| {
            format!(
                "KEY 1 {key} 40001 0\nSCR 4 0 RS1 \"Custom: a.lua\" \"{}\"\n",
                script(dir)
            )
        };
        put(&mac, KEYS, &kb(&mac, 65));
        put(&mac, MOUSE, "[MM_CTX_ITEM]\nmm_0=1\n");
        put(&pc, KEYS, &kb(&pc, 65));
        put(&pc, MOUSE, "[MM_CTX_ITEM]\nmm_0=1\n");
        scan(&mut store, "mac", "MacBook", &mac, &all, 1);
        scan(&mut store, "pc", "Studio PC", &pc, &all, 1);
        back_up(&store, "mac", None, &dir, 1_000).unwrap();
        // The same shortcuts with each machine's own paths are the same.
        let r = sync_in(&mut store, &blobs, "pc", &dir, &mut state, 2).unwrap();
        assert_eq!((r.files_placed(), r.new_conflicts), (0, 0), "{r:?}");

        // A new shortcut and mouse modifier on the Mac come to the PC, with
        // the script path pointing into the PC's folder.
        put(&mac, KEYS, &kb(&mac, 66));
        put(&mac, MOUSE, "[MM_CTX_ITEM]\nmm_0=7\n");
        put(&mac, DRUMS, "drums");
        scan(&mut store, "mac", "MacBook", &mac, &all, 3);
        back_up(&store, "mac", None, &dir, 2_000).unwrap();
        let r = sync_in(&mut store, &blobs, "pc", &dir, &mut state, 4).unwrap();
        // Templates never wait for Reaper.
        assert_eq!(read(&pc, DRUMS).as_deref(), Some("drums"));
        if daw_running(Daw::Reaper) {
            assert!(r.waiting.contains(&"Reaper is open".to_owned()), "{r:?}");
            assert_eq!(read(&pc, KEYS), Some(kb(&pc, 65)));
            return;
        }
        assert!(r.waiting.is_empty(), "{r:?}");
        assert_eq!(read(&pc, KEYS), Some(kb(&pc, 66)));
        assert_eq!(read(&pc, MOUSE).as_deref(), Some("[MM_CTX_ITEM]\nmm_0=7\n"));
        let points = store.restore_points("pc").unwrap();
        assert!(
            points.iter().any(|p| p.daw.as_deref() == Some("reaper")
                && p.label == "Kept in sync: 2 Reaper files from MacBook"),
            "{points:?}"
        );
        // Placed once: nothing more to do.
        let r = sync_in(&mut store, &blobs, "pc", &dir, &mut state, 5).unwrap();
        assert_eq!((r.files_placed(), r.new_conflicts), (0, 0), "{r:?}");
    }

    #[test]
    fn collection_names_renamed_on_one_machine_are_renamed_on_the_other() {
        use patchpilot_parity::ableton::{ABLETON_PREFS, LIBRARY_CFG, set_collection_names};
        if crate::apps::ableton_live_running() {
            return;
        }
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/parity/ableton/prefs-mac/Live 12.1.5/Library.cfg");
        let cfg = std::fs::read_to_string(fixture).unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let mac = tmp.path().join("Mac Live 12.1.5");
        let pc = tmp.path().join("PC Live 12.4.2");
        let dir = tmp.path().join(backup::FOLDER_NAME);
        let blobs = tmp.path().join("blobs");
        put(&mac, LIBRARY_CFG, &cfg);
        // The PC's own User Library path stays whatever the names do.
        let pc_cfg = cfg.replace("/Users/alice", "D:/Dropbox");
        put(&pc, LIBRARY_CFG, &pc_cfg);
        let mut store = Store::open_in_memory().unwrap();
        let rels: &[&str] = &[LIBRARY_CFG];
        scan_roots(
            &mut store,
            "mac",
            "MacBook",
            &[(ABLETON_PREFS, &mac, rels)],
            1,
        );
        scan_roots(
            &mut store,
            "pc",
            "Studio PC",
            &[(ABLETON_PREFS, &pc, rels)],
            1,
        );
        back_up(&store, "mac", None, &dir, 1_000).unwrap();
        let mut state = SyncState::default();
        let r = sync_in(&mut store, &blobs, "pc", &dir, &mut state, 2).unwrap();
        assert_eq!((r.files_placed(), r.new_conflicts), (0, 0), "{r:?}");

        let renamed = set_collection_names(&cfg, "1\tKicks\n2\tSynths & Keys\n").unwrap();
        put(&mac, LIBRARY_CFG, &renamed);
        scan_roots(
            &mut store,
            "mac",
            "MacBook",
            &[(ABLETON_PREFS, &mac, rels)],
            3,
        );
        back_up(&store, "mac", None, &dir, 2_000).unwrap();
        let r = sync_in(&mut store, &blobs, "pc", &dir, &mut state, 4).unwrap();
        assert_eq!((r.files_placed(), r.new_conflicts), (1, 0), "{r:?}");
        assert_eq!(r.placed[0].kinds, ["Collection names and Places"]);
        let now = read(&pc, LIBRARY_CFG).unwrap();
        assert!(now.contains(r#"Uri="color:colors=1" DisplayName="Kicks""#));
        assert!(now.contains("D:/Dropbox/Music/Ableton/User Library"));
        assert!(!now.contains("/Users/alice"));
    }

    #[test]
    fn places_in_dropbox_reach_the_other_machine_and_missing_ones_are_listed() {
        use patchpilot_parity::ableton::{ABLETON_PREFS, LIBRARY_CFG, PlaceRoot, places};
        if crate::apps::ableton_live_running() {
            return;
        }
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/parity/ableton/prefs-mac/Live 12.1.5/Library.cfg");
        let cfg = std::fs::read_to_string(fixture).unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let mac = tmp.path().join("Mac Live 12.1.5");
        let pc = tmp.path().join("PC Live 12.4.2");
        let dir = tmp.path().join(backup::FOLDER_NAME);
        let blobs = tmp.path().join("blobs");
        let slash = |p: PathBuf| p.to_string_lossy().replace('\\', "/");
        let mac_dropbox = slash(tmp.path().join("Mac Dropbox"));
        let pc_dropbox = slash(tmp.path().join("PC Dropbox"));
        std::fs::create_dir_all(format!("{pc_dropbox}/Samples/JUNGLE BREAKS")).unwrap();
        let clouds = |path: &str| {
            crate::TEST_CLOUDS.with(|c| {
                *c.borrow_mut() = Some(vec![PlaceRoot {
                    name: "Dropbox".into(),
                    path: path.into(),
                }])
            })
        };
        put(&mac, LIBRARY_CFG, &cfg);
        put(&pc, LIBRARY_CFG, &cfg.replace("/Users/alice", "D:/Music"));
        let mut store = Store::open_in_memory().unwrap();
        let rels: &[&str] = &[LIBRARY_CFG];
        let scan = |store: &mut Store, id: &str, name: &str, d: &Path, at| {
            scan_roots(store, id, name, &[(ABLETON_PREFS, d, rels)], at)
        };
        scan(&mut store, "mac", "MacBook", &mac, 1);
        scan(&mut store, "pc", "Studio PC", &pc, 1);
        clouds(&mac_dropbox);
        back_up(&store, "mac", None, &dir, 1_000).unwrap();
        clouds(&pc_dropbox);
        // From before Places travelled: what was recorded is forgotten, and
        // the copies are the same, so they're in sync.
        let mut state = SyncState::default();
        state.synced.insert(
            library_cfg_key(),
            ContentHash::try_from(format!("blake3:{}", "0".repeat(64))).unwrap(),
        );
        let r = sync_in(&mut store, &blobs, "pc", &dir, &mut state, 2).unwrap();
        assert_eq!((r.files_placed(), r.new_conflicts), (0, 0), "{r:?}");
        assert_eq!(state.version, STATE_VERSION);

        // On the Mac, two Places in Dropbox are added.
        let with_places = cfg.replace(
            r#"<UserFolderInfo Id="100" Path="/Users/alice/Samples/Breaks" DisplayName="Breaks" IconName="" />"#,
            &format!(
                r#"<UserFolderInfo Id="100" Path="/Users/alice/Samples/Breaks" DisplayName="Breaks" IconName="" />
			<UserFolderInfo Id="101" Path="{mac_dropbox}/Samples/JUNGLE BREAKS" DisplayName="JUNGLE BREAKS" IconName="" />
			<UserFolderInfo Id="102" Path="{mac_dropbox}/Acapellas" DisplayName="Acapellas" IconName="" />"#
            ),
        );
        put(&mac, LIBRARY_CFG, &with_places);
        scan(&mut store, "mac", "MacBook", &mac, 3);
        clouds(&mac_dropbox);
        back_up(&store, "mac", None, &dir, 2_000).unwrap();
        clouds(&pc_dropbox);
        let r = sync_in(&mut store, &blobs, "pc", &dir, &mut state, 4).unwrap();
        assert_eq!((r.files_placed(), r.new_conflicts), (1, 0), "{r:?}");
        let now = read(&pc, LIBRARY_CFG).unwrap();
        let got: Vec<(String, String)> =
            places(&now).into_iter().map(|p| (p.path, p.name)).collect();
        assert_eq!(
            got,
            [
                // The PC's own Place on another drive stays.
                ("D:/Music/Samples/Breaks".to_owned(), "Breaks".to_owned()),
                (
                    format!("{pc_dropbox}/Samples/JUNGLE BREAKS"),
                    "JUNGLE BREAKS".to_owned()
                ),
                (format!("{pc_dropbox}/Acapellas"), "Acapellas".to_owned()),
            ]
        );
        // Acapellas isn't in the PC's Dropbox yet.
        assert_eq!(r.missing_places, ["Acapellas"]);
        // Both now share the same, so the next look finds nothing to do.
        let r = sync_in(&mut store, &blobs, "pc", &dir, &mut state, 5).unwrap();
        assert_eq!((r.files_placed(), r.new_conflicts), (0, 0), "{r:?}");
        crate::TEST_CLOUDS.with(|c| *c.borrow_mut() = None);
    }

    #[test]
    fn a_library_cfg_that_would_change_nothing_here_is_in_sync() {
        use patchpilot_parity::ableton::{ABLETON_PREFS, LIBRARY_CFG, PlaceRoot};
        if crate::apps::ableton_live_running() {
            return;
        }
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/parity/ableton/prefs-mac/Live 12.1.5/Library.cfg");
        let cfg = std::fs::read_to_string(fixture).unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let mac = tmp.path().join("Mac Live");
        let pc = tmp.path().join("PC Live");
        let dir = tmp.path().join(backup::FOLDER_NAME);
        let blobs = tmp.path().join("blobs");
        let mac_dropbox = tmp
            .path()
            .join("Dropbox")
            .to_string_lossy()
            .replace('\\', "/");
        let clouds = |c: Option<&str>| {
            crate::TEST_CLOUDS.with(|t| {
                *t.borrow_mut() = Some(
                    c.map(|path| PlaceRoot {
                        name: "Dropbox".into(),
                        path: path.into(),
                    })
                    .into_iter()
                    .collect(),
                )
            })
        };
        // The Mac has a Place in Dropbox; the PC has no Dropbox to put it in.
        let with_place = cfg.replace(
            "</UserFolderInfoList>",
            &format!(
                "\t<UserFolderInfo Id=\"101\" Path=\"{mac_dropbox}/Breaks\" DisplayName=\"Breaks 2\" IconName=\"\" />\n\t\t</UserFolderInfoList>"
            ),
        );
        put(&mac, LIBRARY_CFG, &with_place);
        put(&pc, LIBRARY_CFG, &cfg);
        let mut store = Store::open_in_memory().unwrap();
        let rels: &[&str] = &[LIBRARY_CFG];
        scan_roots(
            &mut store,
            "mac",
            "MacBook",
            &[(ABLETON_PREFS, &mac, rels)],
            1,
        );
        scan_roots(
            &mut store,
            "pc",
            "Studio PC",
            &[(ABLETON_PREFS, &pc, rels)],
            1,
        );
        clouds(Some(&mac_dropbox));
        back_up(&store, "mac", None, &dir, 1_000).unwrap();
        clouds(None);
        let mut state = SyncState::default();
        state.synced.insert(
            library_cfg_key(),
            hash_here(Some(&pc), ABLETON_PREFS, LIBRARY_CFG).unwrap(),
        );
        state.seen.insert("mac".into(), BTreeMap::new());
        state.version = STATE_VERSION;
        let r = sync_in(&mut store, &blobs, "pc", &dir, &mut state, 2).unwrap();
        // Nothing to write and nothing to ask: only the Place, which can't come.
        assert_eq!((r.files_placed(), r.new_conflicts), (0, 0), "{r:?}");
        assert_eq!(read(&pc, LIBRARY_CFG).unwrap(), cfg);
        assert_eq!(r.missing_places, ["Breaks 2"]);
        crate::TEST_CLOUDS.with(|c| *c.borrow_mut() = None);
    }

    #[test]
    fn renamed_and_moved_files_are_renamed_here_and_undo_puts_them_back() {
        let tmp = tempfile::tempdir().unwrap();
        let mac = tmp.path().join("Mac REAPER");
        let pc = tmp.path().join("PC REAPER");
        let dir = tmp.path().join(backup::FOLDER_NAME);
        let blobs = tmp.path().join("blobs");
        const BUS: &str = "TrackTemplates/Drums Bus.RTrackTemplate";
        const NESTED: &str = "FXChains/Old/Vocal Bus.RfxChain";
        const EDITED: &str = "FXChains/Keys.RfxChain";
        const KEYS_NEW: &str = "FXChains/Keys Bus.RfxChain";
        let all = [DRUMS, BUS, VOCAL, NESTED, EDITED, KEYS_NEW];
        let mut store = Store::open_in_memory().unwrap();
        let mut state = SyncState::default();
        for m in [&mac, &pc] {
            put(m, DRUMS, "drums");
            put(m, NESTED, "vocal");
            put(m, EDITED, "keys");
        }
        scan(&mut store, "mac", "MacBook", &mac, &all, 1);
        scan(&mut store, "pc", "Studio PC", &pc, &all, 1);
        back_up(&store, "mac", None, &dir, 1_000).unwrap();
        sync_in(&mut store, &blobs, "pc", &dir, &mut state, 2).unwrap();

        // Renamed, moved up a folder, and renamed after an edit here.
        std::fs::rename(path(&mac, DRUMS), path(&mac, BUS)).unwrap();
        std::fs::rename(path(&mac, NESTED), path(&mac, VOCAL)).unwrap();
        std::fs::rename(path(&mac, EDITED), path(&mac, KEYS_NEW)).unwrap();
        put(&pc, EDITED, "keys edited on the pc");
        scan(&mut store, "mac", "MacBook", &mac, &all, 3);
        back_up(&store, "mac", None, &dir, 2_000).unwrap();
        let r = sync_in(&mut store, &blobs, "pc", &dir, &mut state, 4).unwrap();
        assert_eq!(r.new_conflicts, 0, "{r:?}");
        assert_eq!(read(&pc, BUS).as_deref(), Some("drums"));
        assert!(
            read(&pc, DRUMS).is_none(),
            "no second copy under the old name"
        );
        assert_eq!(read(&pc, VOCAL).as_deref(), Some("vocal"));
        assert!(read(&pc, NESTED).is_none());
        assert!(
            !path(&pc, "FXChains/Old").exists(),
            "emptied folder removed"
        );
        assert!(path(&pc, "FXChains").is_dir());
        // An edit here is never removed; the new name comes over beside it.
        assert_eq!(read(&pc, EDITED).as_deref(), Some("keys edited on the pc"));
        assert_eq!(read(&pc, KEYS_NEW).as_deref(), Some("keys"));
        assert_eq!(
            r.placed.iter().map(|p| p.renamed).sum::<usize>(),
            2,
            "{r:?}"
        );

        // Nothing more to do, and the old names don't come back.
        let r = sync_in(&mut store, &blobs, "pc", &dir, &mut state, 5).unwrap();
        assert_eq!((r.files_placed(), r.new_conflicts), (0, 0), "{r:?}");
        assert!(read(&pc, DRUMS).is_none());

        // Undo puts the old names back and takes the new ones away.
        let point = store.restore_points("pc").unwrap()[0].clone();
        assert!(
            point
                .label
                .starts_with("Kept in sync: 3 Reaper files from MacBook, 2 renamed"),
            "{}",
            point.label
        );
        crate::restore::undo(&mut store, &blobs, point.id, 6).unwrap();
        assert_eq!(read(&pc, DRUMS).as_deref(), Some("drums"));
        assert_eq!(read(&pc, NESTED).as_deref(), Some("vocal"));
        assert!(read(&pc, BUS).is_none());
    }

    #[test]
    fn a_single_rename_says_so_and_waits_for_nothing_already_here() {
        let tmp = tempfile::tempdir().unwrap();
        let mac = tmp.path().join("Mac REAPER");
        let pc = tmp.path().join("PC REAPER");
        let dir = tmp.path().join(backup::FOLDER_NAME);
        let blobs = tmp.path().join("blobs");
        const BUS: &str = "TrackTemplates/Drums Bus.RTrackTemplate";
        let all = [DRUMS, BUS];
        let mut store = Store::open_in_memory().unwrap();
        let mut state = SyncState::default();
        put(&mac, DRUMS, "drums");
        put(&pc, DRUMS, "drums");
        scan(&mut store, "mac", "MacBook", &mac, &all, 1);
        scan(&mut store, "pc", "Studio PC", &pc, &all, 1);
        back_up(&store, "mac", None, &dir, 1_000).unwrap();
        sync_in(&mut store, &blobs, "pc", &dir, &mut state, 2).unwrap();

        std::fs::rename(path(&mac, DRUMS), path(&mac, BUS)).unwrap();
        // The PC already has the new name too (copied by hand).
        put(&pc, BUS, "drums");
        scan(&mut store, "mac", "MacBook", &mac, &all, 3);
        back_up(&store, "mac", None, &dir, 2_000).unwrap();
        let r = sync_in(&mut store, &blobs, "pc", &dir, &mut state, 4).unwrap();
        assert_eq!(r.placed.len(), 1, "{r:?}");
        assert_eq!((r.placed[0].files, r.placed[0].renamed), (0, 1));
        assert!(read(&pc, DRUMS).is_none());
        assert_eq!(
            store.restore_points("pc").unwrap()[0].label,
            "Kept in sync: Drums.RTrackTemplate renamed to Drums Bus.RTrackTemplate, as on MacBook"
        );
    }

    #[test]
    fn a_deleted_file_is_not_mistaken_for_a_rename_later() {
        let tmp = tempfile::tempdir().unwrap();
        let mac = tmp.path().join("Mac REAPER");
        let pc = tmp.path().join("PC REAPER");
        let dir = tmp.path().join(backup::FOLDER_NAME);
        let blobs = tmp.path().join("blobs");
        const COPY: &str = "TrackTemplates/Drums copy.RTrackTemplate";
        let all = [DRUMS, COPY];
        let mut store = Store::open_in_memory().unwrap();
        let mut state = SyncState::default();
        put(&mac, DRUMS, "drums");
        put(&pc, DRUMS, "drums");
        scan(&mut store, "mac", "MacBook", &mac, &all, 1);
        scan(&mut store, "pc", "Studio PC", &pc, &all, 1);
        back_up(&store, "mac", None, &dir, 1_000).unwrap();
        sync_in(&mut store, &blobs, "pc", &dir, &mut state, 2).unwrap();
        // Deleted on the Mac: stays on the PC.
        std::fs::remove_file(path(&mac, DRUMS)).unwrap();
        scan(&mut store, "mac", "MacBook", &mac, &all, 3);
        back_up(&store, "mac", None, &dir, 2_000).unwrap();
        sync_in(&mut store, &blobs, "pc", &dir, &mut state, 4).unwrap();
        assert_eq!(read(&pc, DRUMS).as_deref(), Some("drums"));
        // Much later the same copy is saved under another name there: a new
        // file, not a rename of the one the PC kept.
        put(&mac, COPY, "drums");
        scan(&mut store, "mac", "MacBook", &mac, &all, 5);
        back_up(&store, "mac", None, &dir, 3_000).unwrap();
        sync_in(&mut store, &blobs, "pc", &dir, &mut state, 6).unwrap();
        assert_eq!(read(&pc, DRUMS).as_deref(), Some("drums"));
        assert_eq!(read(&pc, COPY).as_deref(), Some("drums"));
    }
    #[test]
    fn files_synced_by_hand_stay_in_sync_after() {
        let tmp = tempfile::tempdir().unwrap();
        let mac = tmp.path().join("Mac REAPER");
        let pc = tmp.path().join("PC REAPER");
        let dir = tmp.path().join(backup::FOLDER_NAME);
        let blobs = tmp.path().join("blobs");
        let mut store = Store::open_in_memory().unwrap();
        let mut state = SyncState::default();
        put(&mac, DRUMS, "drums v1");
        put(&pc, DRUMS, "drums v1");
        scan(&mut store, "mac", "MacBook", &mac, &[DRUMS], 1);
        scan(&mut store, "pc", "Studio PC", &pc, &[DRUMS], 1);
        back_up(&store, "mac", None, &dir, 1_000).unwrap();
        sync_in(&mut store, &blobs, "pc", &dir, &mut state, 2).unwrap();

        // Changed on the Mac, and copied here by hand (Sync with).
        put(&mac, DRUMS, "drums v2");
        scan(&mut store, "mac", "MacBook", &mac, &[DRUMS], 3);
        back_up(&store, "mac", None, &dir, 2_000).unwrap();
        put(&pc, DRUMS, "drums v2");
        let b = backup::latest(&backup::subdirs(&dir)[0]).unwrap();
        note_in_sync(&store, "pc", &b, &mut state).unwrap();

        // The next change on the Mac comes over on its own, not as a choice.
        put(&mac, DRUMS, "drums v3");
        scan(&mut store, "mac", "MacBook", &mac, &[DRUMS], 4);
        back_up(&store, "mac", None, &dir, 3_000).unwrap();
        let r = sync_in(&mut store, &blobs, "pc", &dir, &mut state, 5).unwrap();
        assert_eq!((r.files_placed(), r.new_conflicts), (1, 0), "{r:?}");
        assert_eq!(read(&pc, DRUMS).as_deref(), Some("drums v3"));
    }
}
