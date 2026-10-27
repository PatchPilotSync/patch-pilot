//! Back up everything (decision #67): every user-owned file Patch Pilot
//! tracks on this machine (Reaper templates, FX chains, scripts, themes, key
//! maps and menus, Ableton templates and settings, plug-in presets) and the
//! Installer Vault's index go into `Patch Pilot Backup` in a cloud folder the
//! user picks (the same list as the big-file hand-off, decision #64), or any
//! folder. Patch Pilot never talks to the cloud service: its app syncs it.
//!
//! The layout is meant to be read by a person:
//!
//! ```text
//! Patch Pilot Backup/
//!   Studio PC/
//!     Files/Reaper/FXChains/Vocal Bus.RfxChain     the latest copy of each file
//!     Files/Ableton/User Library/Templates/...
//!     Files/Presets/FabFilter presets/...
//!     Files/Installer Vault/vault.json
//!     Versions/2026-10-26 02.48.38/Reaper/...      copies a later backup replaced
//!     Backups/2026-10-26 02.48.38.json             what each backup held
//!     README.txt
//! ```
//!
//! Each backup copies only files that changed since the last one; the copy
//! it replaces (or a file that's gone from the machine) moves to `Versions`,
//! so every earlier backup can still be restored in full. Nothing here is
//! ever deleted by Patch Pilot. Each machine writes only its own folder, so
//! the sync app never has two machines writing one file.
//!
//! A restore lists a backup's files the way Bring over lists the Primary
//! Rig's (paths mapped for this OS, files that work together kept together)
//! and writes them through restore points that History can undo.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use patchpilot_core::{ContentHash, ItemKind, Manifest, Os, TargetProfile};
use patchpilot_parity::ableton::{ABLETON_PREFS, ABLETON_USER_LIBRARY};
use patchpilot_parity::presets::{is_preset_item, is_preset_root};
use patchpilot_parity::reaper::REAPER_RESOURCES;
use patchpilot_store::Store;
use serde::{Deserialize, Serialize};

use crate::bring::{self, FileToBring};
use crate::restore::{ChangeSet, FileChange, apply_changes};

/// The folder Patch Pilot keeps inside the folder the user picked.
pub const FOLDER_NAME: &str = "Patch Pilot Backup";
const SETTINGS_FILE: &str = "backup.json";
/// [`Backup::format`]; a newer one is listed but not restored.
pub const FORMAT_VERSION: u32 = 1;
const FILES: &str = "Files";
const VERSIONS: &str = "Versions";
const BACKUPS: &str = "Backups";
/// The root the Installer Vault's index is backed up under.
pub const VAULT_ROOT: &str = "INSTALLER_VAULT";
const VAULT_INDEX: &str = patchpilot_vault::INDEX_FILE;
/// Only small user-owned files (CLAUDE.md rule 2), as restore points keep.
pub const MAX_BACKUP_BYTES: u64 = crate::transfer::MAX_TRANSFER_BYTES;

/// This machine's backup settings, kept in app-data. Mirrored in `app/src/api.ts`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupSettings {
    /// A [`crate::transfer::CloudFolder::id`], or `custom`; `None` until picked.
    #[serde(default)]
    pub service: Option<String>,
    #[serde(default)]
    pub folder: Option<PathBuf>,
    /// Back up after every scan that found a change.
    #[serde(default)]
    pub auto: bool,
    /// Keep files in sync (decision #70): back up after every change, and
    /// place new and changed files from linked machines' backups here.
    #[serde(default)]
    pub keep_in_sync: bool,
}

impl BackupSettings {
    /// Back up after every scan that found a change: asked for, or needed
    /// to keep files in sync.
    pub fn backs_up_on_change(&self) -> bool {
        self.auto || self.keep_in_sync
    }
}

pub fn load_settings(app_dir: &Path) -> BackupSettings {
    std::fs::read(app_dir.join(SETTINGS_FILE))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

pub fn save_settings(app_dir: &Path, settings: &BackupSettings) -> Result<(), String> {
    std::fs::create_dir_all(app_dir).map_err(|e| e.to_string())?;
    let json = serde_json::to_vec_pretty(settings).expect("settings serialize");
    write_atomic(&app_dir.join(SETTINGS_FILE), &json)
}

/// `Patch Pilot Backup` inside the folder picked; `None` until one is picked
/// or while it's gone (signed out, a drive not connected).
pub fn backup_dir(settings: &BackupSettings) -> Option<PathBuf> {
    settings
        .folder
        .as_ref()
        .filter(|f| f.is_dir())
        .map(|f| f.join(FOLDER_NAME))
}

/// One file in a backup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackedUpFile {
    /// "Reaper", "Ableton", "Presets", "Installer Vault".
    pub app: String,
    pub kind: String,
    pub root: String,
    pub rel: String,
    /// Where it is below `Files/` (and `Versions/<stamp>/`), `/`-separated.
    pub path: String,
    /// Of the bytes kept: for a settings file, only its shareable part, as
    /// Bring over sends it (key maps spelled portably).
    pub hash: ContentHash,
    pub size: u64,
}

impl BackedUpFile {
    pub fn key(&self) -> String {
        format!("{}/{}", self.root, self.rel)
    }
}

/// What one backup held: `Backups/<stamp>.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Backup {
    pub format: u32,
    pub made_at: i64,
    /// `2026-10-26 02.48.38` (UTC): the file's name, and its `Versions` folder.
    pub stamp: String,
    pub machine_id: String,
    pub machine_name: String,
    pub os: Os,
    pub files: Vec<BackedUpFile>,
    /// The machine's scan, narrowed to the files backed up (metadata only),
    /// so a restore can list them the way Bring over does.
    pub scan: Manifest,
    /// The kinds this machine keeps to itself (decision #80): linked
    /// machines don't take its copies of them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub not_synced: Vec<String>,
    /// The machine's folder in the backup.
    #[serde(skip)]
    pub folder: PathBuf,
}

/// A backup in a list. Mirrored in `app/src/api.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BackupInfo {
    pub stamp: String,
    pub made_at: i64,
    pub files: usize,
    pub bytes: u64,
}

/// One machine's backups, newest first. Mirrored in `app/src/api.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BackupSet {
    /// The machine's folder name in the backup.
    pub folder: String,
    pub machine_id: String,
    pub machine_name: String,
    pub os: Os,
    pub backups: Vec<BackupInfo>,
}

/// What [`back_up`] did. Mirrored in `app/src/api.ts`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct BackupReport {
    /// New or changed files copied in.
    pub copied: usize,
    /// Earlier copies moved to `Versions` (changed, or gone from this machine).
    pub versioned: usize,
    /// Files in the backup.
    pub files: usize,
    /// Files that couldn't be read, or are too big to keep.
    pub skipped: Vec<SkippedFile>,
    /// The backup now newest: a new one, or the last one if nothing changed.
    pub stamp: Option<String>,
    /// Nothing changed since the last backup, so none was made.
    pub unchanged: bool,
}

/// A file left out of a backup, and why. Mirrored in `app/src/api.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SkippedFile {
    pub name: String,
    /// Where it is on this machine.
    pub path: String,
    /// Its size, when it was left out for being over [`MAX_BACKUP_BYTES`].
    pub too_big: Option<u64>,
    /// Why it couldn't be read, otherwise.
    pub error: Option<String>,
}

impl SkippedFile {
    fn new(path: &Path, too_big: Option<u64>, error: Option<String>) -> Self {
        SkippedFile {
            name: path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            path: path.display().to_string(),
            too_big,
            error,
        }
    }
}

/// Back up this machine (`this`) into `dir` (`Patch Pilot Backup`), from its
/// latest scan, plus the Installer Vault's index in `vault_root`.
pub fn back_up(
    store: &Store,
    this: &str,
    vault_root: Option<&Path>,
    dir: &Path,
    now: i64,
) -> Result<BackupReport, String> {
    back_up_with(store, this, vault_root, dir, &[], now)
}

/// [`back_up`], saying which kinds this machine keeps to itself
/// (`not_synced`, see [`Backup::not_synced`]).
pub fn back_up_with(
    store: &Store,
    this: &str,
    vault_root: Option<&Path>,
    dir: &Path,
    not_synced: &[String],
    now: i64,
) -> Result<BackupReport, String> {
    let scan = store
        .latest_manifest(this)
        .map_err(|e| e.to_string())?
        .ok_or("Scan this machine first.")?;
    let (mut current, scan, skipped) = collect(&scan);
    if let Some(root) = vault_root
        && let Ok(bytes) = std::fs::read(root.join(VAULT_INDEX))
    {
        current.push((
            BackedUpFile {
                app: "Installer Vault".into(),
                kind: "Index".into(),
                root: VAULT_ROOT.into(),
                rel: VAULT_INDEX.into(),
                path: format!("Installer Vault/{VAULT_INDEX}"),
                hash: patchpilot_sync::content_hash(&bytes),
                size: bytes.len() as u64,
            },
            bytes,
        ));
    }

    let folder = machine_folder(dir, &scan.machine.id, &scan.machine.name)?;
    let prev = latest(&folder);
    let mut report = BackupReport {
        files: current.len(),
        skipped,
        ..BackupReport::default()
    };
    let same_list = prev.as_ref().is_some_and(|p| {
        let a: HashSet<_> = p.files.iter().map(|f| (&f.path, &f.hash)).collect();
        let b: HashSet<_> = current.iter().map(|(f, _)| (&f.path, &f.hash)).collect();
        a == b && p.not_synced == not_synced
    });
    let files_dir = folder.join(FILES);
    let all_there = current.iter().all(|(f, _)| {
        std::fs::metadata(below(&files_dir, &f.path)).is_ok_and(|m| m.len() == f.size)
    });
    if same_list && all_there {
        report.unchanged = true;
        report.stamp = prev.map(|p| p.stamp);
        return Ok(report);
    }

    let (stamp, made_at) = free_stamp(&folder, now);
    let versions = folder.join(VERSIONS).join(&stamp);
    let prev_hash: HashMap<&str, &ContentHash> = prev
        .iter()
        .flat_map(|p| &p.files)
        .map(|f| (f.path.as_str(), &f.hash))
        .collect();
    for (f, bytes) in &current {
        let target = below(&files_dir, &f.path);
        if let Ok(meta) = std::fs::metadata(&target) {
            let same = if prev_hash.get(f.path.as_str()) == Some(&&f.hash) {
                meta.len() == f.size
            } else {
                std::fs::read(&target).is_ok_and(|b| patchpilot_sync::content_hash(&b) == f.hash)
            };
            if same {
                continue;
            }
            move_into(&target, &below(&versions, &f.path))?;
            report.versioned += 1;
        }
        write_atomic(&target, bytes)?;
        report.copied += 1;
    }
    // Files gone from this machine stay restorable from `Versions`.
    let now_paths: BTreeSet<&str> = current.iter().map(|(f, _)| f.path.as_str()).collect();
    for f in prev.iter().flat_map(|p| &p.files) {
        let target = below(&files_dir, &f.path);
        if !now_paths.contains(f.path.as_str()) && target.is_file() {
            move_into(&target, &below(&versions, &f.path))?;
            remove_empty_parents(&target, &files_dir);
            report.versioned += 1;
        }
    }

    let backup = Backup {
        format: FORMAT_VERSION,
        made_at,
        stamp: stamp.clone(),
        machine_id: scan.machine.id.clone(),
        machine_name: scan.machine.name.clone(),
        os: scan.machine.os,
        files: current.into_iter().map(|(f, _)| f).collect(),
        scan,
        not_synced: not_synced.to_vec(),
        folder: folder.clone(),
    };
    let json = serde_json::to_vec_pretty(&backup).map_err(|e| e.to_string())?;
    write_atomic(&folder.join(BACKUPS).join(format!("{stamp}.json")), &json)?;
    let readme = folder.join("README.txt");
    if !readme.exists() {
        write_atomic(&readme, README.as_bytes())?;
    }
    report.stamp = Some(stamp);
    Ok(report)
}

const README: &str = "\
Patch Pilot backup of this machine's templates, scripts, themes, settings,
presets and Installer Vault index. Plug-ins and sample libraries aren't here:
they need their installers and licences.

Files     the latest copy of each file, by app and kind
Versions  earlier copies, in a folder named for the backup that replaced them
Backups   what each backup held (times are UTC)

Restore from Patch Pilot (This machine > Backup > Restore), which puts each
file back in the right place on Windows or macOS and lets you undo it in
History. Patch Pilot never deletes anything here; delete old Versions
yourself if you need the space.
";

type WithBytes = (BackedUpFile, Vec<u8>);

/// The files to back up from `scan`, with their bytes; the scan narrowed to
/// them; and the paths that couldn't be backed up.
fn collect(scan: &Manifest) -> (Vec<WithBytes>, Manifest, Vec<SkippedFile>) {
    let presets = preset_folder_names(scan);
    let mut out = Vec::new();
    let mut skipped = Vec::new();
    let mut kept = scan.clone();
    kept.items.clear();
    kept.products
        .retain(|p| p.canonical_id.0.starts_with("presets/"));
    for item in &scan.items {
        if is_preset_item(item) {
            kept.items.push(item.clone());
            continue;
        }
        if item.kind == ItemKind::Folder {
            continue;
        }
        let Some((shared, root, rel, kind)) = bring::classify(&item.path) else {
            continue;
        };
        let Some(path) = scan.machine.path_map.resolve(&item.path).map(PathBuf::from) else {
            continue;
        };
        if let Some(size) = item.size.filter(|&s| s > MAX_BACKUP_BYTES) {
            skipped.push(SkippedFile::new(&path, Some(size), None));
            continue;
        }
        let bytes = match std::fs::read(&path) {
            Ok(b) if b.len() as u64 <= MAX_BACKUP_BYTES => b,
            Ok(b) => {
                skipped.push(SkippedFile::new(&path, Some(b.len() as u64), None));
                continue;
            }
            // Gone since the scan: the next scan and backup catch up.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                skipped.push(SkippedFile::new(&path, None, Some(e.to_string())));
                continue;
            }
        };
        let Some(base) = path.ancestors().nth(rel.split('/').count()) else {
            continue;
        };
        let bytes = (shared.view)(base, rel, bytes);
        let hash = patchpilot_sync::content_hash(&bytes);
        let mut item = item.clone();
        item.content_hash = Some(hash.clone());
        item.size = Some(bytes.len() as u64);
        kept.items.push(item);
        out.push((
            BackedUpFile {
                app: shared.app.into(),
                kind: kind.into(),
                root: root.into(),
                rel: rel.into(),
                path: shown_path(root, rel, &presets),
                hash,
                size: bytes.len() as u64,
            },
            bytes,
        ));
    }
    (out, kept, skipped)
}

/// Each preset root's folder name in the backup: its product's name
/// ("FabFilter presets"), made unique.
fn preset_folder_names(scan: &Manifest) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut used = BTreeSet::new();
    for item in scan.items.iter().filter(|i| is_preset_item(i)) {
        let patchpilot_core::PortablePath::Logical { root, rel } = &item.path else {
            continue;
        };
        if !rel.is_empty() || out.contains_key(root) {
            continue;
        }
        let name = item
            .product_id
            .as_ref()
            .and_then(|id| scan.product(id))
            .map_or_else(|| root.clone(), |p| p.name.clone());
        let mut name = safe_name(&name);
        if !used.insert(name.to_lowercase()) {
            name = format!("{name} {root}");
            used.insert(name.to_lowercase());
        }
        out.insert(root.clone(), name);
    }
    out
}

fn shown_path(root: &str, rel: &str, presets: &BTreeMap<String, String>) -> String {
    match root {
        REAPER_RESOURCES => format!("Reaper/{rel}"),
        ABLETON_USER_LIBRARY => format!("Ableton/User Library/{rel}"),
        ABLETON_PREFS => format!("Ableton/Preferences/{rel}"),
        _ if is_preset_root(root) => {
            let folder = presets
                .get(root)
                .cloned()
                .unwrap_or_else(|| root.to_owned());
            format!("Presets/{folder}/{rel}")
        }
        _ => format!("Other/{root}/{rel}"),
    }
}

/// A name safe as a folder on Windows and macOS.
fn safe_name(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            c if c.is_control() => '-',
            c => c,
        })
        .collect();
    let s = s.trim().trim_end_matches('.').trim().to_owned();
    if s.is_empty() { "Machine".into() } else { s }
}

/// This machine's folder in `dir`: the one its earlier backups are in, else
/// a new one named after it (with part of its ID if another machine has
/// that name).
fn machine_folder(dir: &Path, id: &str, name: &str) -> Result<PathBuf, String> {
    for folder in subdirs(dir) {
        if latest(&folder).is_some_and(|b| b.machine_id == id) {
            return Ok(folder);
        }
    }
    let base = safe_name(name);
    let plain = dir.join(&base);
    let folder = if plain.exists() {
        dir.join(format!("{base} {}", &id[..id.len().min(6)]))
    } else {
        plain
    };
    std::fs::create_dir_all(&folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    Ok(folder)
}

/// The name of the folder in `dir` that the machine `this` backs up into,
/// if it has backed up there.
pub fn own_folder(dir: &Path, this: &str) -> Option<String> {
    subdirs(dir)
        .into_iter()
        .find(|f| latest(f).is_some_and(|b| b.machine_id == this))
        .map(|f| {
            f.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        })
}

pub(crate) fn subdirs(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|e| {
            e.flatten()
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

/// The backups' stamps in `folder`, oldest first.
pub(crate) fn stamps(folder: &Path) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(folder.join(BACKUPS))
        .map(|e| {
            e.flatten()
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().into_owned();
                    let stamp = name.strip_suffix(".json")?;
                    is_stamp(stamp).then(|| stamp.to_owned())
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

fn read_backup(folder: &Path, stamp: &str) -> Option<Backup> {
    try_read_backup(folder, stamp, 1).ok()
}

/// The backup `stamp` in `folder`, or why it can't be read, in words a user
/// can act on. A cloud app can hold a file it's syncing for a moment
/// (Dropbox on Windows does) or hand over a half-downloaded one, so a failed
/// read is tried `tries` times, a short while apart.
fn try_read_backup(folder: &Path, stamp: &str, tries: u32) -> Result<Backup, String> {
    let path = folder.join(BACKUPS).join(format!("{stamp}.json"));
    let mut last = String::new();
    for attempt in 0..tries.max(1) {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_millis(400 * u64::from(attempt)));
        }
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(format!(
                    "The backup from {stamp} isn't in the backup folder anymore. Pick another one."
                ));
            }
            Err(e) => {
                last = format!(
                    "Can't open the backup from {stamp} yet. Your cloud folder's app is probably \
                     still downloading or syncing it; try again in a minute. ({e})"
                );
                continue;
            }
        };
        match serde_json::from_slice::<Backup>(&bytes) {
            Ok(mut b) => {
                b.folder = folder.to_owned();
                return Ok(b);
            }
            // Cut short: the cloud app hasn't finished downloading it.
            Err(e) if e.is_eof() => {
                last = format!(
                    "The backup from {stamp} is still downloading. Try again once your cloud \
                     folder's app has finished syncing."
                );
            }
            Err(e) => {
                return Err(format!(
                    "The backup from {stamp} is damaged, so it can't be used. Pick another one. ({e})"
                ));
            }
        }
    }
    Err(last)
}

/// The newest backup in `folder` that can be read.
pub(crate) fn latest(folder: &Path) -> Option<Backup> {
    stamps(folder)
        .iter()
        .rev()
        .find_map(|s| read_backup(folder, s))
}

/// `YYYY-MM-DD HH.MM.SS` (UTC) for `now`, moved on a second at a time until
/// no backup in `folder` has it.
fn free_stamp(folder: &Path, now: i64) -> (String, i64) {
    let taken = stamps(folder);
    let mut t = now;
    loop {
        let s = stamp(t);
        if !taken.contains(&s) && !folder.join(VERSIONS).join(&s).exists() {
            return (s, t);
        }
        t += 1;
    }
}

/// `YYYY-MM-DD HH.MM.SS` from Unix seconds, UTC.
pub fn stamp(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}.{:02}.{:02}",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

fn is_stamp(s: &str) -> bool {
    s.len() == 19
        && s.bytes().enumerate().all(|(i, b)| match i {
            4 | 7 => b == b'-',
            10 => b == b' ',
            13 | 16 => b == b'.',
            _ => b.is_ascii_digit(),
        })
}

/// `rel` (`/`-separated, from a backup's index) below `base`, never above it.
pub(crate) fn below(base: &Path, rel: &str) -> PathBuf {
    rel.split('/')
        .filter(|p| !p.is_empty() && *p != "." && *p != "..")
        .fold(base.to_owned(), |p, part| p.join(part))
}

pub(crate) fn safe_rel(rel: &str) -> bool {
    !rel.starts_with('/')
        && rel
            .split('/')
            .all(|p| !p.is_empty() && p != "." && p != ".." && !p.contains(['\\', ':']))
}

/// Write via a dot-named file beside `path` and rename, so the sync app only
/// ever uploads whole files.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("no parent folder")?;
    std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let tmp = parent.join(format!(".{name}.patchpilot-partial"));
    std::fs::write(&tmp, bytes).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("{}: {e}", path.display())
    })
}

fn move_into(from: &Path, to: &Path) -> Result<(), String> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    std::fs::rename(from, to).map_err(|e| format!("{}: {e}", from.display()))
}

pub(crate) fn remove_empty_parents(path: &Path, stop: &Path) {
    let mut dir = path.parent();
    while let Some(d) = dir {
        if d == stop || !d.starts_with(stop) || std::fs::remove_dir(d).is_err() {
            return;
        }
        dir = d.parent();
    }
}

/// Every machine's backups in `dir`, the machine `this` first.
pub fn list_backups(dir: &Path, this: &str) -> Vec<BackupSet> {
    #[derive(Deserialize)]
    struct Head {
        made_at: i64,
        stamp: String,
        machine_id: String,
        machine_name: String,
        os: Os,
        files: Vec<Size>,
    }
    #[derive(Deserialize)]
    struct Size {
        size: u64,
    }
    let mut out: Vec<BackupSet> = Vec::new();
    for folder in subdirs(dir) {
        let mut set: Option<BackupSet> = None;
        for s in stamps(&folder).iter().rev() {
            let Some(head) = std::fs::read(folder.join(BACKUPS).join(format!("{s}.json")))
                .ok()
                .and_then(|b| serde_json::from_slice::<Head>(&b).ok())
            else {
                continue;
            };
            let set = set.get_or_insert_with(|| BackupSet {
                folder: folder
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                machine_id: head.machine_id.clone(),
                machine_name: head.machine_name.clone(),
                os: head.os,
                backups: Vec::new(),
            });
            set.backups.push(BackupInfo {
                stamp: head.stamp,
                made_at: head.made_at,
                files: head.files.len(),
                bytes: head.files.iter().map(|f| f.size).sum(),
            });
        }
        out.extend(set);
    }
    out.sort_by_key(|s| s.machine_id != this);
    out
}

/// The backup `stamp` of the machine in `dir/<folder>`.
pub fn load_backup(dir: &Path, folder: &str, stamp: &str) -> Result<Backup, String> {
    if !safe_rel(folder) || folder.contains('/') || !is_stamp(stamp) {
        return Err("Not a backup.".into());
    }
    let b = try_read_backup(&dir.join(folder), stamp, 3)?;
    if b.format > FORMAT_VERSION {
        return Err("That backup was made by a newer Patch Pilot. Update this one first.".into());
    }
    Ok(b)
}

/// What restoring `backup` would put on this machine (its latest scan
/// `machine`): every file it lacks or has a different copy of, sorted and
/// linked as Bring over lists them. The Installer Vault's index is included
/// only when this machine's vault folder (`vault_root`) has none. `profile`
/// leaves out what's ignored on this machine (see [`restore_profile`]).
pub fn restore_list(
    backup: &Backup,
    profile: &TargetProfile,
    machine: Option<&Manifest>,
    vault_root: Option<&Path>,
) -> Vec<FileToBring> {
    let kept: BTreeSet<String> = backup
        .files
        .iter()
        .map(|f| f.key().to_lowercase())
        .collect();
    let files = bring::files_to_bring(&backup.scan, profile, machine)
        .into_iter()
        .filter(|f| kept.contains(&f.key().to_lowercase()))
        .map(|mut f| {
            // Everything in the backup is there to be had, whatever its size.
            f.too_large = false;
            f
        })
        .collect();
    let mut files = bring::drop_same_here(&backup.scan, machine, files);
    if let Some(root) = vault_root
        && !root.join(VAULT_INDEX).exists()
        && let Some(v) = backup.files.iter().find(|f| f.root == VAULT_ROOT)
    {
        files.push(FileToBring {
            app: v.app.clone(),
            kind: v.kind.clone(),
            root: v.root.clone(),
            rel: v.rel.clone(),
            hash: v.hash.clone(),
            size: Some(v.size),
            replaces: false,
            too_large: false,
            product_id: None,
            needs: Vec::new(),
            incomplete: false,
        });
    }
    files
}

/// What a restore onto `this` keeps: everything, less what's ignored there
/// (decision #65), as Bring over from another machine does.
pub fn restore_profile(store: &Store, this: &str) -> Result<TargetProfile, String> {
    let mut profile = TargetProfile::default();
    profile
        .rules
        .extend(store.ignore_rules(this).map_err(|e| e.to_string())?);
    Ok(profile)
}

/// The bytes `backup` held for `f`, checked against its hash: in the
/// `Versions` folder of the first later backup that replaced it, else in
/// `Files`. `None` while the cloud app hasn't downloaded it.
pub(crate) fn bytes_of(backup: &Backup, f: &BackedUpFile) -> Option<Vec<u8>> {
    if !safe_rel(&f.path) {
        return None;
    }
    let later = stamps(&backup.folder)
        .into_iter()
        .filter(|s| *s > backup.stamp)
        .map(|s| backup.folder.join(VERSIONS).join(s));
    later.chain([backup.folder.join(FILES)]).find_map(|base| {
        let bytes = std::fs::read(below(&base, &f.path)).ok()?;
        (patchpilot_sync::content_hash(&bytes) == f.hash).then_some(bytes)
    })
}

/// What [`restore`] did. Mirrored in `app/src/api.ts`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct RestoreReport {
    pub written: usize,
    /// Replaced files, saved first: History can undo the restore.
    pub backed_up: usize,
    pub same: usize,
    /// Files brought because a chosen file needs them.
    pub companions: usize,
    /// Paths the backup should have but that can't be read yet (still
    /// downloading, or removed from the backup folder).
    pub unavailable: Vec<String>,
    /// Paths left behind because a file they need is unavailable.
    pub held_back: Vec<String>,
}

/// Restore the chosen files (`root/rel` keys from [`restore_list`]) and
/// every file they need from `backup` onto this machine (`this`), one
/// restore point per folder.
#[allow(clippy::too_many_arguments)]
pub fn restore(
    store: &mut Store,
    blobs: &Path,
    this: &str,
    backup: &Backup,
    keys: &[String],
    vault_root: Option<&Path>,
    now: i64,
) -> Result<RestoreReport, String> {
    let machine = store.latest_manifest(this).map_err(|e| e.to_string())?;
    let profile = restore_profile(store, this)?;
    let list = restore_list(backup, &profile, machine.as_ref(), vault_root);
    let chosen = bring::with_needs(&list, keys);
    let held_in: HashMap<String, &BackedUpFile> = backup
        .files
        .iter()
        .map(|f| (f.key().to_lowercase(), f))
        .collect();
    let mut report = RestoreReport {
        companions: chosen.iter().filter(|f| !keys.contains(&f.key())).count(),
        ..RestoreReport::default()
    };
    let mut fetched: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut bad = BTreeSet::new();
    for f in &chosen {
        match held_in
            .get(&f.key().to_lowercase())
            .and_then(|b| bytes_of(backup, b))
        {
            Some(bytes) => {
                fetched.insert(f.key(), bytes);
            }
            None => {
                bad.insert(f.key());
                report.unavailable.push(f.rel.clone());
            }
        }
    }
    let held = bring::blocked(&list, &bad);
    // (root, app) to the files to write there.
    let mut by_root: BTreeMap<(String, String), Vec<bring::Fetched>> = BTreeMap::new();
    for f in chosen {
        if held.contains(&f.key()) {
            report.held_back.push(f.rel.clone());
            continue;
        }
        if let Some(bytes) = fetched.remove(&f.key()) {
            by_root
                .entry((f.root.clone(), f.app.clone()))
                .or_default()
                .push((f.rel.clone(), bytes));
        }
    }
    let from = format!(
        "the backup of {} from {}",
        backup.machine_name, backup.stamp
    );
    let count = |n: usize, what: &str| format!("{n} {what}{}", if n == 1 { "" } else { "s" });
    for ((root, app), files) in by_root {
        if root == VAULT_ROOT {
            let Some(vault) = vault_root else { continue };
            let req = ChangeSet {
                machine_id: this.to_owned(),
                label: format!("Restored the Installer Vault index from {from}"),
                daw: None,
                changes: files
                    .into_iter()
                    .map(|(_, bytes)| FileChange {
                        path: vault.join(VAULT_INDEX),
                        contents: Some(bytes),
                    })
                    .collect(),
                now,
            };
            let point = apply_changes(store, blobs, &req).map_err(|e| e.to_string())?;
            report.written += point.changed.len();
            continue;
        }
        let dir = bring::root_dir(machine.as_ref(), &root).ok_or_else(|| {
            format!("Can't find {app}'s folder on this machine. Open {app} once, then try again.")
        })?;
        let files: Vec<(String, Vec<u8>)> = match Os::current() {
            Some(os) => files
                .into_iter()
                .map(|(rel, bytes)| {
                    let bytes = bring::adapt_for_here(&backup.scan, &root, &rel, &dir, os, bytes);
                    (rel, bytes)
                })
                .collect(),
            None => files,
        };
        let label = format!(
            "Restored {} from {from}",
            count(files.len(), &format!("{app} file"))
        );
        let r = bring::write_files(store, blobs, this, &label, &root, &dir, &files, now)?;
        report.written += r.written;
        report.backed_up += r.backed_up;
        report.same += r.same;
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use patchpilot_core::{Arch, Confidence, Item, MachineInfo, PathMap, PortablePath};
    use patchpilot_store::SnapshotReason;

    fn template(rel: &str, size: usize) -> Item {
        Item {
            kind: ItemKind::DawAsset,
            product_id: None,
            format: None,
            platform: Os::Macos,
            arch: None,
            version: None,
            path: PortablePath::logical(REAPER_RESOURCES, rel),
            content_hash: None,
            size: Some(size as u64),
            mtime: None,
            platform_portable: true,
            confidence: Confidence::High,
            requires: Vec::new(),
            needs_files: Vec::new(),
        }
    }

    /// A scan of the Reaper folder `dir`, of the files in it now.
    fn scan(id: &str, name: &str, dir: &Path, rels: &[&str]) -> Manifest {
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
            0,
        );
        m.items = rels
            .iter()
            .map(|r| {
                let len = std::fs::metadata(below(dir, r)).map_or(0, |m| m.len() as usize);
                template(r, len)
            })
            .collect();
        m
    }

    fn put(dir: &Path, rel: &str, body: &str) {
        let p = below(dir, rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }

    const DRUMS: &str = "TrackTemplates/Drums.RTrackTemplate";
    const VOCAL: &str = "FXChains/Vocal Bus.RfxChain";

    #[test]
    fn says_which_files_were_too_big_and_how_big() {
        let tmp = tempfile::tempdir().unwrap();
        let reaper = tmp.path().join("REAPER");
        put(&reaper, DRUMS, "drums");
        put(&reaper, VOCAL, "vocal");
        let mut m = scan("pc", "Studio PC", &reaper, &[DRUMS, VOCAL]);
        m.items[1].size = Some(MAX_BACKUP_BYTES + 1);
        let mut store = Store::open_in_memory().unwrap();
        store.save_manifest(&m, SnapshotReason::Scan, 1).unwrap();

        let dir = tmp.path().join("Dropbox").join(FOLDER_NAME);
        let r = back_up(&store, "pc", None, &dir, 1_761_446_918).unwrap();
        assert_eq!(r.files, 1);
        let [f] = r.skipped.as_slice() else {
            panic!("{:?}", r.skipped)
        };
        assert_eq!(f.name, "Vocal Bus.RfxChain");
        // The fixture's path map is macOS's, so separators differ on Windows.
        assert_eq!(
            PathBuf::from(&f.path).canonicalize().unwrap(),
            below(&reaper, VOCAL).canonicalize().unwrap()
        );
        assert_eq!(
            (f.too_big, f.error.as_deref()),
            (Some(MAX_BACKUP_BYTES + 1), None)
        );
    }

    #[test]
    fn backs_up_only_changes_and_keeps_every_version() {
        let tmp = tempfile::tempdir().unwrap();
        let reaper = tmp.path().join("REAPER");
        let vault = tmp.path().join("Vault");
        let dir = tmp.path().join("Dropbox").join(FOLDER_NAME);
        put(&reaper, DRUMS, "drums v1");
        put(&reaper, VOCAL, "vocal");
        put(&vault, VAULT_INDEX, r#"{"version":1,"entries":[]}"#);
        let mut store = Store::open_in_memory().unwrap();
        let save = |store: &mut Store, rels: &[&str], t| {
            store
                .save_manifest(
                    &scan("pc", "Studio PC", &reaper, rels),
                    SnapshotReason::Scan,
                    t,
                )
                .unwrap();
        };
        save(&mut store, &[DRUMS, VOCAL], 1);

        let first = back_up(&store, "pc", Some(&vault), &dir, 1_761_446_918).unwrap();
        assert_eq!((first.copied, first.versioned, first.files), (3, 0, 3));
        assert_eq!(first.stamp.as_deref(), Some("2025-10-26 02.48.38"));
        let pc = dir.join("Studio PC");
        let read = |p: PathBuf| std::fs::read_to_string(p).unwrap();
        assert_eq!(read(pc.join("Files/Reaper").join(DRUMS)), "drums v1");
        assert!(pc.join("Files/Installer Vault/vault.json").is_file());
        assert!(pc.join("Backups/2025-10-26 02.48.38.json").is_file());
        assert!(pc.join("README.txt").is_file());

        // Nothing changed: no new backup.
        let again = back_up(&store, "pc", Some(&vault), &dir, 1_761_446_999).unwrap();
        assert!(again.unchanged);
        assert_eq!(stamps(&pc).len(), 1);

        // One file changed, one gone: only those move.
        put(&reaper, DRUMS, "drums v2");
        std::fs::remove_file(below(&reaper, VOCAL)).unwrap();
        save(&mut store, &[DRUMS], 2);
        let second = back_up(&store, "pc", Some(&vault), &dir, 1_761_450_000).unwrap();
        assert_eq!((second.copied, second.versioned), (1, 2));
        let s2 = second.stamp.unwrap();
        assert_eq!(read(pc.join("Files/Reaper").join(DRUMS)), "drums v2");
        assert_eq!(
            read(pc.join(VERSIONS).join(&s2).join("Reaper").join(DRUMS)),
            "drums v1"
        );
        assert!(
            !pc.join("Files/Reaper/FXChains").exists(),
            "empty folders go"
        );
        // No temp files left behind.
        assert!(walk(&dir).iter().all(|p| !p.contains("patchpilot-partial")));

        let sets = list_backups(&dir, "pc");
        assert_eq!(sets.len(), 1);
        assert_eq!(sets[0].machine_name, "Studio PC");
        assert_eq!(sets[0].backups.len(), 2);
        assert_eq!(sets[0].backups[0].stamp, s2, "newest first");

        // Roll back onto a fresh Mac with its own Reaper folder: the first
        // backup comes back whole, through a restore point.
        let mac_reaper = tmp.path().join("Mac REAPER");
        let mac_vault = tmp.path().join("Mac Vault");
        std::fs::create_dir_all(&mac_vault).unwrap();
        store
            .save_manifest(
                &scan("mac", "MacBook", &mac_reaper, &[]),
                SnapshotReason::FirstScan,
                3,
            )
            .unwrap();
        let old = load_backup(&dir, "Studio PC", "2025-10-26 02.48.38").unwrap();
        let list = restore_list(
            &old,
            &TargetProfile::default(),
            store.latest_manifest("mac").unwrap().as_ref(),
            Some(&mac_vault),
        );
        let keys: Vec<String> = list.iter().map(FileToBring::key).collect();
        assert_eq!(keys.len(), 3, "{keys:?}");
        let blobs = tmp.path().join("blobs");
        let r = restore(&mut store, &blobs, "mac", &old, &keys, Some(&mac_vault), 4).unwrap();
        assert_eq!(r.written, 3, "{r:?}");
        assert!(r.unavailable.is_empty());
        assert_eq!(read(below(&mac_reaper, DRUMS)), "drums v1");
        assert_eq!(read(below(&mac_reaper, VOCAL)), "vocal");
        assert!(mac_vault.join(VAULT_INDEX).is_file());
        assert_eq!(store.restore_points("mac").unwrap().len(), 2);

        // A second machine with the same name gets its own folder.
        store
            .save_manifest(
                &scan("pc2", "Studio PC", &reaper, &[DRUMS]),
                SnapshotReason::FirstScan,
                5,
            )
            .unwrap();
        back_up(&store, "pc2", None, &dir, 1_761_460_000).unwrap();
        assert_eq!(list_backups(&dir, "pc2").len(), 2);
        assert_eq!(list_backups(&dir, "pc2")[0].folder, "Studio PC pc2");
    }

    #[test]
    fn a_file_missing_from_the_backup_holds_back_what_needs_it() {
        let tmp = tempfile::tempdir().unwrap();
        let reaper = tmp.path().join("REAPER");
        let dir = tmp.path().join(FOLDER_NAME);
        put(&reaper, "Scripts/Tool.lua", "dofile('lib.lua')");
        put(&reaper, "Scripts/lib.lua", "lib");
        let mut m = scan(
            "pc",
            "PC",
            &reaper,
            &["Scripts/Tool.lua", "Scripts/lib.lua"],
        );
        m.items[0].needs_files = vec!["Scripts/lib.lua".into()];
        let mut store = Store::open_in_memory().unwrap();
        store
            .save_manifest(&m, SnapshotReason::FirstScan, 1)
            .unwrap();
        let s = back_up(&store, "pc", None, &dir, 10)
            .unwrap()
            .stamp
            .unwrap();
        std::fs::remove_file(dir.join("PC/Files/Reaper/Scripts/lib.lua")).unwrap();

        let here = tmp.path().join("here");
        store
            .save_manifest(
                &scan("new", "New", &here, &[]),
                SnapshotReason::FirstScan,
                2,
            )
            .unwrap();
        let b = load_backup(&dir, "PC", &s).unwrap();
        let r = restore(
            &mut store,
            &tmp.path().join("blobs"),
            "new",
            &b,
            &["REAPER_RESOURCES/Scripts/Tool.lua".into()],
            None,
            3,
        )
        .unwrap();
        assert_eq!(r.companions, 1);
        assert_eq!(r.unavailable, ["Scripts/lib.lua"]);
        assert_eq!(r.held_back, ["Scripts/Tool.lua"]);
        assert!(!here.join("Scripts").exists());
    }

    #[test]
    fn refuses_paths_outside_the_backup() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(load_backup(tmp.path(), "../x", "2025-10-26 02.48.38").is_err());
        assert!(load_backup(tmp.path(), "PC", "../../etc").is_err());
        assert_eq!(
            below(tmp.path(), "../a/./b"),
            tmp.path().join("a").join("b")
        );
        assert!(!safe_rel("Reaper/../../x"));
        assert!(is_stamp(&stamp(0)));
        assert_eq!(stamp(0), "1970-01-01 00.00.00");
    }

    #[test]
    fn unreadable_backups_say_why() {
        let tmp = tempfile::tempdir().unwrap();
        let backups = tmp.path().join("PC").join(BACKUPS);
        std::fs::create_dir_all(&backups).unwrap();
        let err = |stamp: &str| load_backup(tmp.path(), "PC", stamp).unwrap_err();

        assert!(err("2026-10-26 15.44.56").contains("isn't in the backup folder anymore"));
        // Half downloaded by the cloud app.
        std::fs::write(
            backups.join("2026-10-26 15.44.57.json"),
            br#"{"format": 1, "made_at""#,
        )
        .unwrap();
        assert!(err("2026-10-26 15.44.57").contains("still downloading"));
        std::fs::write(
            backups.join("2026-10-26 15.44.58.json"),
            b"{\"format\": \"one\"}",
        )
        .unwrap();
        assert!(err("2026-10-26 15.44.58").contains("damaged"));
    }

    #[test]
    fn settings_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(load_settings(tmp.path()), BackupSettings::default());
        let s = BackupSettings {
            service: Some("dropbox".into()),
            folder: Some(tmp.path().to_owned()),
            auto: true,
            keep_in_sync: false,
        };
        save_settings(tmp.path(), &s).unwrap();
        assert_eq!(load_settings(tmp.path()), s);
        assert_eq!(backup_dir(&s), Some(tmp.path().join(FOLDER_NAME)));
    }

    fn walk(dir: &Path) -> Vec<String> {
        let mut out = Vec::new();
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                out.extend(walk(&p));
            } else {
                out.push(p.to_string_lossy().into_owned());
            }
        }
        out
    }
}
