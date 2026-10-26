//! Big-file hand-off (decision #64): Bring over files too big for the relay
//! travel through a folder the user's own cloud app keeps in step:
//! `Patch Pilot Transfers` in the Dropbox, OneDrive, Google Drive, iCloud
//! Drive, Box, pCloud, Sync.com or MEGA folder each machine picks, or any
//! folder (a NAS share). Patch Pilot never talks to those services: their
//! apps sync the folder.
//!
//! The machine that has a file puts it there when the user says so. A machine
//! that needs it places it through a restore point on its own (the user sent
//! it on purpose), and any machine deletes a transfer once no linked machine
//! needs it any more, so nothing piles up. Each file travels under a name
//! made from its path and size, the same on every machine, and is never
//! written twice, so the sync app has no reason to make conflicted copies.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use patchpilot_core::{Manifest, Os, PortablePath};
use patchpilot_store::Store;
use serde::{Deserialize, Serialize};

use crate::bring::{self, FileToBring};

/// The folder Patch Pilot keeps inside the user's cloud folder.
pub const FOLDER_NAME: &str = "Patch Pilot Transfers";
/// Only small user-owned files (rule 2): restore points keep files up to this.
pub const MAX_TRANSFER_BYTES: u64 = 64 * 1024 * 1024;
const SETTINGS_FILE: &str = "transfers.json";
/// [`TransferSettings::service`] for a folder the user picked.
pub const CUSTOM: &str = "custom";

/// A cloud service's local folder on this machine. Mirrored in `app/src/api.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CloudFolder {
    /// `dropbox`, `onedrive`, `google-drive`, `icloud`, `box`, `pcloud`,
    /// `sync`, `mega`, or `other` (another macOS File Provider).
    pub id: String,
    /// "Dropbox", "OneDrive (Contoso)", ...
    pub name: String,
    pub path: PathBuf,
}

/// Where cloud apps keep their folders on this machine.
#[derive(Debug, Clone, Default)]
pub struct CloudEnv {
    pub home: PathBuf,
    /// Windows `%APPDATA%` and `%LOCALAPPDATA%`.
    pub app_data: Option<PathBuf>,
    pub local_app_data: Option<PathBuf>,
    /// Windows `%OneDrive%`, `%OneDriveConsumer%`, `%OneDriveCommercial%`.
    pub onedrive: Vec<PathBuf>,
    /// Windows drive roots (`G:\`), where Google Drive mounts.
    pub drives: Vec<PathBuf>,
}

impl CloudEnv {
    pub fn from_env() -> Option<Self> {
        let var = |k: &str| {
            std::env::var_os(k)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
        };
        let windows = Os::current()? == Os::Windows;
        Some(Self {
            home: PathBuf::from(crate::home_dir()?),
            app_data: var("APPDATA"),
            local_app_data: var("LOCALAPPDATA"),
            onedrive: ["OneDrive", "OneDriveConsumer", "OneDriveCommercial"]
                .into_iter()
                .filter_map(var)
                .collect(),
            drives: if windows {
                (b'D'..=b'Z')
                    .map(|l| PathBuf::from(format!("{}:\\", l as char)))
                    .collect()
            } else {
                Vec::new()
            },
        })
    }
}

/// Every cloud folder found on this machine, in the order the list shows
/// them: Dropbox, OneDrive, Google Drive, iCloud Drive, Box, pCloud,
/// Sync.com, MEGA, then any other macOS File Provider folder
/// (`~/Library/CloudStorage`).
pub fn detect_clouds(os: Os, env: &CloudEnv) -> Vec<CloudFolder> {
    let home = &env.home;
    let mut out: Vec<CloudFolder> = Vec::new();
    let mut add = |id: &str, name: &str, path: PathBuf| {
        if path.is_dir() && !out.iter().any(|c| c.path == path) {
            out.push(CloudFolder {
                id: id.into(),
                name: name.into(),
                path,
            });
        }
    };
    // macOS keeps File Provider folders here: `OneDrive-Personal`,
    // `GoogleDrive-a@b.com`, `Box-Box`, `Dropbox`, ...
    let cloud_storage = match os {
        Os::Macos => sorted_dirs(&home.join("Library").join("CloudStorage")),
        Os::Windows => Vec::new(),
    };
    let provider = |prefix: &str| -> Vec<PathBuf> {
        cloud_storage
            .iter()
            .filter(|p| file_name(p).starts_with(prefix))
            .cloned()
            .collect()
    };

    for p in dropbox_folder(os, env)
        .into_iter()
        .chain(provider("Dropbox"))
        .chain([home.join("Dropbox")])
    {
        add("dropbox", "Dropbox", p);
    }

    let onedrive = match os {
        Os::Windows => env.onedrive.clone(),
        Os::Macos => provider("OneDrive"),
    };
    for p in onedrive.into_iter().chain([home.join("OneDrive")]) {
        let n = file_name(&p);
        let account = n
            .strip_prefix("OneDrive - ")
            .or_else(|| n.strip_prefix("OneDrive-"))
            .filter(|a| *a != "Personal");
        let name = account.map_or_else(|| "OneDrive".to_owned(), |a| format!("OneDrive ({a})"));
        add("onedrive", &name, p);
    }

    for p in provider("GoogleDrive")
        .into_iter()
        .chain(env.drives.iter().cloned())
        .map(|p| p.join("My Drive"))
        .chain([home.join("Google Drive")])
    {
        add("google-drive", "Google Drive", p);
    }

    let icloud = match os {
        Os::Macos => home
            .join("Library")
            .join("Mobile Documents")
            .join("com~apple~CloudDocs"),
        Os::Windows => home.join("iCloudDrive"),
    };
    add("icloud", "iCloud Drive", icloud);

    for p in provider("Box").into_iter().chain([home.join("Box")]) {
        add("box", "Box", p);
    }
    add("pcloud", "pCloud", home.join("pCloud Drive"));
    add("pcloud", "pCloud", home.join("pCloudDrive"));
    add("sync", "Sync.com", home.join("Sync"));
    add("mega", "MEGA", home.join("MEGA"));
    add("mega", "MEGA", home.join("MEGAsync"));
    add("mega", "MEGA", home.join("Documents").join("MEGAsync"));

    // Any other File Provider (Nextcloud, Proton Drive, ...), by its folder name.
    for p in &cloud_storage {
        if ["Dropbox", "OneDrive", "GoogleDrive", "Box"]
            .iter()
            .any(|known| file_name(p).starts_with(known))
        {
            continue;
        }
        let name = file_name(p)
            .split('-')
            .next()
            .unwrap_or_default()
            .to_owned();
        add("other", &name, p.clone());
    }
    out
}

fn sorted_dirs(dir: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|e| {
            e.flatten()
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect()
        })
        .unwrap_or_default();
    dirs.sort();
    dirs
}

fn file_name(p: &Path) -> &str {
    p.file_name().and_then(|n| n.to_str()).unwrap_or_default()
}

/// The Dropbox folder the Dropbox app reports in its `info.json`
/// (`~/.dropbox/info.json` on macOS, `%APPDATA%` or `%LOCALAPPDATA%\Dropbox\info.json`
/// on Windows): the personal one, else the business one, if it exists.
fn dropbox_folder(os: Os, env: &CloudEnv) -> Option<PathBuf> {
    let candidates: Vec<PathBuf> = match os {
        Os::Macos => vec![env.home.join(".dropbox").join("info.json")],
        Os::Windows => [&env.app_data, &env.local_app_data]
            .into_iter()
            .flatten()
            .map(|b| b.join("Dropbox").join("info.json"))
            .collect(),
    };
    candidates.iter().find_map(|info| {
        let json: serde_json::Value = serde_json::from_slice(&std::fs::read(info).ok()?).ok()?;
        ["personal", "business"].into_iter().find_map(|account| {
            let dir = PathBuf::from(json.get(account)?.get("path")?.as_str()?);
            dir.is_dir().then_some(dir)
        })
    })
}

/// This machine's cloud folders.
pub fn this_machines_clouds() -> Vec<CloudFolder> {
    match (Os::current(), CloudEnv::from_env()) {
        (Some(os), Some(env)) => detect_clouds(os, &env),
        _ => Vec::new(),
    }
}

/// This machine's choice, kept in app-data, never in the user's folders.
/// Each machine picks its own, since paths differ. Mirrored in `app/src/api.ts`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferSettings {
    /// A [`CloudFolder::id`], or [`CUSTOM`]; `None` until the user picks.
    pub service: Option<String>,
    /// The cloud folder picked (one service can have several accounts) or,
    /// for [`CUSTOM`], any folder, such as a NAS share.
    pub folder: Option<PathBuf>,
}

pub fn load_settings(app_dir: &Path) -> TransferSettings {
    std::fs::read(app_dir.join(SETTINGS_FILE))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

pub fn save_settings(app_dir: &Path, settings: &TransferSettings) -> Result<(), String> {
    std::fs::create_dir_all(app_dir).map_err(|e| e.to_string())?;
    let json = serde_json::to_vec_pretty(settings).expect("settings serialize");
    let tmp = app_dir.join(format!("{SETTINGS_FILE}.tmp"));
    std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, app_dir.join(SETTINGS_FILE)).map_err(|e| e.to_string())
}

/// Where transfers go on this machine: `Patch Pilot Transfers` inside the
/// folder picked. `None` until one is picked, or while it's gone (a cloud
/// app signed out, a NAS not mounted).
pub fn transfer_dir(settings: &TransferSettings) -> Option<PathBuf> {
    settings
        .folder
        .as_ref()
        .filter(|f| f.is_dir())
        .map(|f| f.join(FOLDER_NAME))
}

/// The name a file travels under, e.g. `3f2a9c1e04b7d5a6-Huge.ffp`: the same
/// for that path and size on every machine; a changed file gets a new one.
pub fn transfer_name(root: &str, rel: &str, size: u64) -> String {
    let key = format!("{root}\0{}\0{size}", rel.to_ascii_lowercase());
    let id = blake3::hash(key.as_bytes()).to_hex();
    let file = rel.rsplit('/').next().unwrap_or(rel);
    format!("{}-{file}", &id[..16])
}

/// Names Patch Pilot made in the transfer folder, including copies a sync
/// app made of them, and the half-written files `send` leaves if cut off.
fn is_transfer_name(name: &str) -> bool {
    let name = name.strip_prefix('.').unwrap_or(name);
    name.len() > 17
        && name.as_bytes()[16] == b'-'
        && name[..16].bytes().all(|b| b.is_ascii_hexdigit())
}

/// A big file a linked machine needs that this machine has. Mirrored in `app/src/api.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Sendable {
    pub root: String,
    pub rel: String,
    pub size: u64,
    /// The machines that need it, by name.
    pub machines: Vec<String>,
    /// Already in the transfer folder, waiting to be picked up.
    pub sent: bool,
    #[serde(skip)]
    pub path: PathBuf,
}

/// What each linked machine (other than `except`) still needs from the
/// Primary Rig that's too big for the relay but small enough to hand off,
/// by machine name.
fn needed(store: &Store, except: &str) -> Result<Vec<(String, FileToBring)>, String> {
    let Some(golden) = store.golden(None).map_err(err)? else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for info in store.machines().map_err(err)? {
        if info.id == except {
            continue;
        }
        let Some(scan) = store.latest_manifest(&info.id).map_err(err)? else {
            continue;
        };
        let profile = store
            .machine_profile(&info.id)
            .map_err(err)?
            .unwrap_or_default();
        out.extend(
            bring::files_to_bring(&golden.manifest, &profile, Some(&scan))
                .into_iter()
                .filter(handed_off)
                .map(|f| (info.name.clone(), f)),
        );
    }
    Ok(out)
}

fn handed_off(f: &FileToBring) -> bool {
    f.too_large && f.size.is_some_and(|s| s <= MAX_TRANSFER_BYTES)
}

/// The big files other linked machines need that this machine (`this`) has
/// the same copy of (same path and size), so it can send them. `dir` is the
/// transfer folder, if any, to tell which are sent already.
pub fn sendable(store: &Store, this: &str, dir: Option<&Path>) -> Result<Vec<Sendable>, String> {
    let Some(here) = store.latest_manifest(this).map_err(err)? else {
        return Ok(Vec::new());
    };
    let mut out: BTreeMap<String, Sendable> = BTreeMap::new();
    for (machine, f) in needed(store, this)? {
        let size = f.size.unwrap_or_default();
        let Some(path) = local_copy(&here, &f.root, &f.rel, size) else {
            continue;
        };
        let name = transfer_name(&f.root, &f.rel, size);
        let entry = out.entry(name.clone()).or_insert_with(|| Sendable {
            sent: dir.is_some_and(|d| d.join(&name).is_file()),
            root: f.root.clone(),
            rel: f.rel.clone(),
            size,
            machines: Vec::new(),
            path,
        });
        if !entry.machines.contains(&machine) {
            entry.machines.push(machine);
        }
    }
    Ok(out.into_values().collect())
}

/// Where this machine keeps the file at `root`/`rel` with that size, per its scan.
fn local_copy(here: &Manifest, root: &str, rel: &str, size: u64) -> Option<PathBuf> {
    here.items
        .iter()
        .find(|i| {
            i.size == Some(size)
                && matches!(&i.path, PortablePath::Logical { root: r, rel: p }
                    if r == root && p.eq_ignore_ascii_case(rel))
        })
        .and_then(|i| here.machine.path_map.resolve(&i.path))
        .map(PathBuf::from)
}

/// Copy files into the transfer folder `dir`, each under its
/// [`transfer_name`]: written beside it under a dot name, then renamed, so
/// the sync app only ever uploads whole files. Files already there and files
/// that changed since the scan are skipped. Returns how many were copied.
pub fn send(dir: &Path, files: &[Sendable]) -> Result<usize, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut copied = 0;
    for f in files {
        let name = transfer_name(&f.root, &f.rel, f.size);
        let target = dir.join(&name);
        if target.is_file() {
            continue;
        }
        let bytes = std::fs::read(&f.path).map_err(|e| format!("{}: {e}", f.path.display()))?;
        if bytes.len() as u64 != f.size {
            continue;
        }
        let partial = dir.join(format!(".{name}"));
        std::fs::write(&partial, &bytes).map_err(|e| format!("{}: {e}", partial.display()))?;
        std::fs::rename(&partial, &target).map_err(|e| format!("{}: {e}", target.display()))?;
        copied += 1;
    }
    Ok(copied)
}

/// What [`receive`] did. Mirrored in `app/src/api.ts`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ReceiveReport {
    /// Files written on this machine.
    pub placed: usize,
    /// Files this machine needs that are in the folder but not fully
    /// downloaded yet (online-only, or still syncing).
    pub waiting: usize,
    /// Transfers deleted because no linked machine needs them any more.
    pub cleaned: usize,
}

/// Place every file this machine (`this`) needs that's waiting in the
/// transfer folder `dir`, through one restore point per folder, then delete
/// the transfers no linked machine still needs. A file is placed only once
/// it is all there: right size and, where known, right hash.
pub fn receive(
    store: &mut Store,
    blobs: &Path,
    this: &str,
    dir: &Path,
    now: i64,
) -> Result<ReceiveReport, String> {
    let mut report = ReceiveReport::default();
    if store.golden(None).map_err(err)?.is_none() {
        return Ok(report);
    }
    let machine = store.latest_manifest(this).map_err(err)?;
    let mine = needed_here(store, this)?;

    let mut placed = BTreeSet::new();
    let mut by_root: BTreeMap<String, Vec<(String, Vec<u8>)>> = BTreeMap::new();
    for f in &mine {
        let size = f.size.unwrap_or_default();
        let name = transfer_name(&f.root, &f.rel, size);
        match fetch(dir, &name, f) {
            Fetch::Absent => {}
            Fetch::Waiting => report.waiting += 1,
            Fetch::Ready(bytes) => {
                placed.insert(name);
                by_root
                    .entry(f.root.clone())
                    .or_default()
                    .push((f.rel.clone(), bytes));
            }
        }
    }
    for (root, files) in by_root {
        let Some(target) = bring::root_dir(machine.as_ref(), &root) else {
            continue;
        };
        let label = format!(
            "Brought over {} file{} through {FOLDER_NAME}",
            files.len(),
            if files.len() == 1 { "" } else { "s" }
        );
        // Files already here as sent (placed before, kept for another
        // machine) aren't written again.
        report.placed +=
            bring::write_files(store, blobs, this, &label, &root, &target, &files, now)?.written;
    }

    // Still wanted: by the other machines, and by this one unless just placed.
    let mut wanted: BTreeSet<String> = needed(store, this)?
        .iter()
        .map(|(_, f)| name_of(f))
        .collect();
    wanted.extend(mine.iter().map(name_of).filter(|n| !placed.contains(n)));
    report.cleaned = clean(dir, &wanted);
    Ok(report)
}

/// The big files this machine needs that can be handed off.
fn needed_here(store: &Store, this: &str) -> Result<Vec<FileToBring>, String> {
    let Some(golden) = store.golden(None).map_err(err)? else {
        return Ok(Vec::new());
    };
    let machine = store.latest_manifest(this).map_err(err)?;
    let profile = store
        .machine_profile(this)
        .map_err(err)?
        .unwrap_or_default();
    Ok(
        bring::files_to_bring(&golden.manifest, &profile, machine.as_ref())
            .into_iter()
            .filter(handed_off)
            .collect(),
    )
}

/// How many big files this machine needs (for the UI's "waiting" line).
pub fn needed_count(store: &Store, this: &str) -> Result<usize, String> {
    Ok(needed_here(store, this)?.len())
}

fn name_of(f: &FileToBring) -> String {
    transfer_name(&f.root, &f.rel, f.size.unwrap_or_default())
}

enum Fetch {
    Absent,
    /// There, but online-only or still arriving.
    Waiting,
    Ready(Vec<u8>),
}

/// Read a transfer if it's all there. iCloud keeps a not-yet-downloaded
/// file as a `.<name>.icloud` stand-in, so that asks iCloud to fetch it;
/// OneDrive, Google Drive and Dropbox online-only files download as they
/// are read, and anything short or with the wrong hash waits for the next
/// look (the folder watch fires when the sync app finishes).
fn fetch(dir: &Path, name: &str, f: &FileToBring) -> Fetch {
    let path = dir.join(name);
    if !path.is_file() {
        let stand_in = dir.join(format!(".{name}.icloud"));
        if !stand_in.is_file() {
            return Fetch::Absent;
        }
        request_download(&path);
        return Fetch::Waiting;
    }
    let size = f.size.unwrap_or_default();
    let Ok(bytes) = std::fs::read(&path) else {
        return Fetch::Waiting;
    };
    let hash_known = f.hash != bring::size_only_hash(size);
    if bytes.len() as u64 != size || (hash_known && patchpilot_sync::content_hash(&bytes) != f.hash)
    {
        return Fetch::Waiting;
    }
    Fetch::Ready(bytes)
}

/// Ask iCloud Drive to download an online-only file (macOS `brctl`).
fn request_download(path: &Path) {
    if Os::current() == Some(Os::Macos) {
        let _ = std::process::Command::new("/usr/bin/brctl")
            .arg("download")
            .arg(path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}

/// A half-written file older than this was cut off and is removed.
const STALE_PARTIAL: std::time::Duration = std::time::Duration::from_secs(60 * 60);

/// Delete Patch Pilot's transfers in `dir` that aren't `wanted`, with any
/// conflicted copies a sync app made of them and iCloud stand-ins for them.
/// Returns how many.
fn clean(dir: &Path, wanted: &BTreeSet<String>) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut cleaned = 0;
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if !is_transfer_name(&name) || !e.path().is_file() {
            continue;
        }
        let remove = match name.strip_prefix('.') {
            Some(rest) => match rest.strip_suffix(".icloud") {
                Some(base) => !wanted.contains(base),
                // A file `send` is still writing, unless it was cut off.
                None => e
                    .metadata()
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.elapsed().ok())
                    .is_some_and(|age| age > STALE_PARTIAL),
            },
            None => !wanted.contains(&name),
        };
        if remove && std::fs::remove_file(e.path()).is_ok() {
            cleaned += 1;
        }
    }
    cleaned
}

fn err(e: patchpilot_store::StoreError) -> String {
    e.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use patchpilot_core::{
        Arch, Confidence, Item, ItemKind, MachineInfo, PathMap, Product, ProductId, TargetProfile,
    };

    const ROOT: &str = "PRESETS_FABFILTER";
    const BIG: u64 = bring::MAX_FILE_BYTES + 10;

    fn machine(id: &str, name: &str, os: Os, dir: &Path, with_file: bool) -> Manifest {
        let mut m = Manifest::new(
            MachineInfo {
                id: id.into(),
                name: name.into(),
                os,
                os_version: None,
                arch: Arch::Aarch64,
                path_map: PathMap::new(os).with_root(ROOT, dir.to_string_lossy()),
            },
            0,
        );
        let item = |rel: &str, kind, size, product: Option<&str>| Item {
            kind,
            product_id: product.map(|p| ProductId(p.into())),
            format: None,
            platform: os,
            arch: None,
            version: None,
            path: PortablePath::logical(ROOT, rel),
            content_hash: None,
            size: Some(size),
            mtime: None,
            platform_portable: true,
            confidence: Confidence::High,
            requires: Vec::new(),
            needs_files: Vec::new(),
        };
        m.products.push(Product {
            canonical_id: ProductId("presets/fabfilter".into()),
            vendor: "FabFilter".into(),
            name: "FabFilter presets".into(),
        });
        let mut folder = item("", ItemKind::Folder, BIG, Some("presets/fabfilter"));
        folder.content_hash = Some(patchpilot_sync::content_hash(id.as_bytes()));
        m.items.push(folder);
        if with_file {
            m.items
                .push(item("Pro-Q 4/Huge.ffp", ItemKind::DawAsset, BIG, None));
        }
        m
    }

    #[test]
    fn a_big_preset_goes_through_the_cloud_folder_and_is_cleaned_up() {
        let tmp = tempfile::tempdir().unwrap();
        let pc_dir = tmp.path().join("pc");
        let mac_dir = tmp.path().join("mac");
        let cloud = tmp.path().join("Dropbox").join(FOLDER_NAME);
        std::fs::create_dir_all(pc_dir.join("Pro-Q 4")).unwrap();
        let bytes = vec![7u8; BIG as usize];
        std::fs::write(pc_dir.join("Pro-Q 4/Huge.ffp"), &bytes).unwrap();

        let mut store = Store::open_in_memory().unwrap();
        let pc = machine("pc", "Studio PC", Os::Macos, &pc_dir, true);
        let mac = machine("mac", "MacBook", Os::Macos, &mac_dir, false);
        store
            .save_manifest(&pc, patchpilot_store::SnapshotReason::FirstScan, 1)
            .unwrap();
        store
            .save_manifest(&mac, patchpilot_store::SnapshotReason::FirstScan, 1)
            .unwrap();
        store.save_golden(&pc, Some("pc"), None, 1).unwrap();
        assert_eq!(
            bring::files_to_bring(&pc, &TargetProfile::default(), Some(&mac)).len(),
            1
        );

        // The PC has it and the Mac needs it.
        let out = sendable(&store, "pc", Some(&cloud)).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].machines, ["MacBook"]);
        assert!(!out[0].sent);
        assert!(sendable(&store, "mac", Some(&cloud)).unwrap().is_empty());
        assert_eq!(send(&cloud, &out).unwrap(), 1);
        assert_eq!(send(&cloud, &out).unwrap(), 0, "never written twice");
        assert!(sendable(&store, "pc", Some(&cloud)).unwrap()[0].sent);
        // The PC keeps it there: the Mac still needs it.
        let blobs = tmp.path().join("blobs");
        let r = receive(&mut store, &blobs, "pc", &cloud, 2).unwrap();
        assert_eq!((r.placed, r.cleaned), (0, 0));

        // A conflicted copy a sync app made, and a cut-off one, go too.
        let name = transfer_name(ROOT, "Pro-Q 4/Huge.ffp", BIG);
        let copy = format!(
            "{} (MacBook's conflicted copy).ffp",
            name.trim_end_matches(".ffp")
        );
        std::fs::write(cloud.join(&copy), b"x").unwrap();
        std::fs::write(cloud.join("notes.txt"), b"mine").unwrap();

        let r = receive(&mut store, &blobs, "mac", &cloud, 2).unwrap();
        assert_eq!((r.placed, r.waiting, r.cleaned), (1, 0, 2));
        assert_eq!(
            std::fs::read(mac_dir.join("Pro-Q 4/Huge.ffp")).unwrap(),
            bytes
        );
        assert!(!cloud.join(&name).exists());
        assert!(cloud.join("notes.txt").exists(), "only Patch Pilot's files");
        assert_eq!(store.restore_points("mac").unwrap().len(), 1);
    }

    #[test]
    fn a_file_still_arriving_waits() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("pc");
        let cloud = tmp.path().join(FOLDER_NAME);
        std::fs::create_dir_all(&cloud).unwrap();
        let mut store = Store::open_in_memory().unwrap();
        let pc = machine("pc", "PC", Os::Macos, &dir, true);
        let mac = machine("mac", "Mac", Os::Macos, &tmp.path().join("mac"), false);
        for m in [&pc, &mac] {
            store
                .save_manifest(m, patchpilot_store::SnapshotReason::FirstScan, 1)
                .unwrap();
        }
        store.save_golden(&pc, Some("pc"), None, 1).unwrap();
        let name = transfer_name(ROOT, "Pro-Q 4/Huge.ffp", BIG);
        std::fs::write(cloud.join(&name), b"only part of it").unwrap();
        let blobs = tmp.path().join("blobs");
        let r = receive(&mut store, &blobs, "mac", &cloud, 2).unwrap();
        assert_eq!((r.placed, r.waiting, r.cleaned), (0, 1, 0));
        // An iCloud stand-in for it waits too.
        std::fs::remove_file(cloud.join(&name)).unwrap();
        std::fs::write(cloud.join(format!(".{name}.icloud")), b"").unwrap();
        let r = receive(&mut store, &blobs, "mac", &cloud, 2).unwrap();
        assert_eq!((r.placed, r.waiting, r.cleaned), (0, 1, 0));
    }

    #[test]
    fn finds_cloud_folders_in_list_order() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        let dropbox = home.join("Work Dropbox");
        for d in [
            dropbox.clone(),
            home.join("Library/CloudStorage/OneDrive-Personal"),
            home.join("Library/CloudStorage/OneDrive-Contoso"),
            home.join("Library/CloudStorage/GoogleDrive-a@b.com/My Drive"),
            home.join("Library/CloudStorage/Nextcloud-cloud.example.org"),
            home.join("Library/Mobile Documents/com~apple~CloudDocs"),
            home.join("Sync"),
            home.join("MEGA"),
        ] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::create_dir_all(home.join(".dropbox")).unwrap();
        std::fs::write(
            home.join(".dropbox/info.json"),
            format!(
                r#"{{"business": {{"path": {:?}, "host": 1}}}}"#,
                dropbox.to_string_lossy()
            ),
        )
        .unwrap();
        let env = CloudEnv {
            home: home.to_owned(),
            ..CloudEnv::default()
        };
        let got: Vec<(String, String)> = detect_clouds(Os::Macos, &env)
            .into_iter()
            .map(|c| (c.id, c.name))
            .collect();
        let want = [
            ("dropbox", "Dropbox"),
            ("onedrive", "OneDrive (Contoso)"),
            ("onedrive", "OneDrive"),
            ("google-drive", "Google Drive"),
            ("icloud", "iCloud Drive"),
            ("sync", "Sync.com"),
            ("mega", "MEGA"),
            ("other", "Nextcloud"),
        ];
        assert_eq!(got, want.map(|(a, b)| (a.to_owned(), b.to_owned())));

        // Windows: OneDrive from its variable, Google Drive on its drive letter.
        let g = home.join("G");
        std::fs::create_dir_all(g.join("My Drive")).unwrap();
        std::fs::create_dir_all(home.join("iCloudDrive")).unwrap();
        let win = CloudEnv {
            home: home.to_owned(),
            onedrive: vec![home.join("Library/CloudStorage/OneDrive-Contoso")],
            drives: vec![g.clone()],
            ..CloudEnv::default()
        };
        let ids: Vec<String> = detect_clouds(Os::Windows, &win)
            .into_iter()
            .map(|c| c.id)
            .collect();
        assert_eq!(ids, ["onedrive", "google-drive", "icloud", "sync", "mega"]);
    }

    #[test]
    fn names_are_stable_and_recognised() {
        let a = transfer_name(ROOT, "Pro-Q 4/Huge.ffp", 5);
        assert_eq!(a[..16], transfer_name(ROOT, "pro-q 4/huge.ffp", 5)[..16]);
        assert_ne!(a, transfer_name(ROOT, "Pro-Q 4/Huge.ffp", 6));
        assert!(a.ends_with("-Huge.ffp"));
        assert!(is_transfer_name(&a));
        assert!(is_transfer_name(&format!(".{a}.icloud")));
        assert!(!is_transfer_name("Huge.ffp"));
    }

    #[test]
    fn settings_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(load_settings(tmp.path()), TransferSettings::default());
        let s = TransferSettings {
            service: Some(CUSTOM.into()),
            folder: Some(tmp.path().to_owned()),
        };
        save_settings(tmp.path(), &s).unwrap();
        assert_eq!(load_settings(tmp.path()), s);
        assert_eq!(transfer_dir(&s), Some(tmp.path().join(FOLDER_NAME)));
        let gone = TransferSettings {
            folder: Some(tmp.path().join("unmounted")),
            ..s
        };
        assert_eq!(transfer_dir(&gone), None);
    }
}
