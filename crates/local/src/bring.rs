//! Bring over (decision #45): copy the Primary Rig's user-owned files, such as
//! Reaper templates, FX chains, scripts and themes, and plug-in presets
//! (decision #63), onto this machine. Never plug-ins (CLAUDE.md
//! rule 2), never silently: the user picks the files, anything replaced is
//! backed up to app-data first, and each file is staged and renamed into place.
//!
//! No network here: the app fetches each file's bytes from the relay, sealed
//! under the rig key and checked against the hash Primary Rig recorded.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use patchpilot_core::{ContentHash, Item, Manifest, Os, PortablePath, ProductId, TargetProfile};
use patchpilot_parity::ableton::{
    ABLETON_PREFS, ABLETON_USER_LIBRARY, LIBRARY_CFG, merge_incoming, prefs_label, shared_view,
    user_library_label,
};
use patchpilot_parity::apply_profile;
use patchpilot_parity::presets::{self, ROOT_PREFIX, is_preset_file, is_preset_root};
use patchpilot_parity::reaper::{
    REAPER_RESOURCES, ResourceKind, portable_key_map, reaper_resource_dir, resource_kind,
};
use patchpilot_store::Store;
use serde::Serialize;

use crate::restore::{ChangeSet, Daw, FileChange, apply_changes};

/// The relay keeps objects up to 2 MB; leave room for the envelope.
pub const MAX_FILE_BYTES: u64 = 1_900_000;

/// See [`SharedRoot::merge`].
pub type MergeFn = fn(&str, Option<&[u8]>, Vec<u8>) -> Result<Vec<u8>, String>;

/// A logical root whose files may be brought over, and how.
pub struct SharedRoot {
    pub root: &'static str,
    /// Shown as the group heading: "Reaper".
    pub app: &'static str,
    /// The label for a `/`-separated path below the root ("FX chain"), or
    /// `None` for paths never shared or written, including unsafe ones.
    pub kind: fn(&str) -> Option<&'static str>,
    /// Where the root is on this machine when its last scan didn't map it.
    pub default_dir: fn() -> Option<PathBuf>,
    /// Checked before writing under the root, e.g. "Close the DAW first"
    /// for its settings (CLAUDE.md rule 5).
    pub guard: fn() -> Result<(), String>,
    /// The bytes others get for a file here (the root's folder, `rel`, its
    /// bytes): the file itself, or for a settings file only its shareable
    /// part. Hashed at scan time the same way.
    pub view: fn(&Path, &str, Vec<u8>) -> Vec<u8>,
    /// What to write given what's there now (`rel`, current bytes, incoming
    /// bytes): the incoming file, or the current one with the shared part
    /// merged in.
    pub merge: MergeFn,
    /// The DAW a restore point is tagged with; `Some` also makes
    /// [`apply_changes`] refuse while that DAW runs.
    pub daw: Option<Daw>,
    /// A DAW that must be closed before writing this path, because it
    /// rewrites the file itself (CLAUDE.md rule 5), for roots whose other
    /// files may be written while it runs.
    pub closed_daw: fn(&str) -> Option<Daw>,
}

/// Every root Bring over knows. Add one entry per app's root.
pub const SHARED_ROOTS: &[SharedRoot] = &[
    SharedRoot {
        root: REAPER_RESOURCES,
        app: "Reaper",
        kind: |rel| resource_kind(rel).map(|k| k.label()),
        default_dir: default_reaper_dir,
        // Templates, scripts and themes aren't Reaper's settings, and Reaper
        // reads them when they are used, so writing them while it runs is safe.
        guard: || Ok(()),
        // Key maps travel with this machine's resource folder spelled portably.
        view: |dir, rel, bytes| match resource_kind(rel) {
            Some(ResourceKind::KeyMap) => portable_key_map(bytes, dir, this_os()),
            _ => bytes,
        },
        merge: |_, _, incoming| Ok(incoming),
        daw: None,
        // Key maps and menus are Reaper's own settings: only while it's closed.
        closed_daw: |rel| {
            resource_kind(rel)
                .is_some_and(|k| k.needs_reaper_closed())
                .then_some(Daw::Reaper)
        },
    },
    // Ableton (decision #41): Live indexes the User Library and reads
    // Options.txt at launch, so both wait until Live is closed.
    SharedRoot {
        root: ABLETON_USER_LIBRARY,
        app: "Ableton",
        kind: user_library_label,
        default_dir: || crate::standard_ableton_dirs()?.user_library,
        guard: || crate::apps::write_guard(ABLETON_USER_LIBRARY),
        view: |_, _, bytes| bytes,
        merge: |_, _, incoming| Ok(incoming),
        daw: Some(Daw::Ableton),
        closed_daw: |_| None,
    },
    SharedRoot {
        root: ABLETON_PREFS,
        app: "Ableton",
        kind: prefs_label,
        default_dir: || crate::standard_ableton_dirs()?.prefs,
        guard: || crate::apps::write_guard(ABLETON_PREFS),
        // Library.cfg shares only its Collection names and the Places in
        // cloud folders, merged into this machine's own file so its paths and
        // other Places stay (decision #79).
        view: |_, rel, bytes| {
            if rel.eq_ignore_ascii_case(LIBRARY_CFG) {
                shared_view(ABLETON_PREFS, rel, bytes, &crate::place_clouds())
            } else {
                bytes
            }
        },
        merge: |rel, current, incoming| {
            let clouds = if rel.eq_ignore_ascii_case(LIBRARY_CFG) {
                crate::place_clouds()
            } else {
                Vec::new()
            };
            merge_incoming(ABLETON_PREFS, rel, current, incoming, &clouds, this_os())
        },
        daw: Some(Daw::Ableton),
        closed_daw: |_| None,
    },
    // Plug-in presets (decision #63): this one entry stands for every
    // `PRESETS_<VENDOR>` root. Plug-ins read a preset when it is opened, so
    // writing them while a DAW runs is safe.
    SharedRoot {
        root: ROOT_PREFIX,
        app: "Presets",
        kind: |rel| presets::is_shareable_file(rel).then_some("Preset"),
        // Each vendor has its own place: see `root_dir`.
        default_dir: || None,
        guard: || Ok(()),
        view: |_, _, bytes| bytes,
        merge: |_, _, incoming| Ok(incoming),
        daw: None,
        closed_daw: |_| None,
    },
];

/// The OS paths here are spelled for (macOS's `/` where neither applies, as
/// in tests on Linux).
fn this_os() -> Os {
    Os::current().unwrap_or(Os::Macos)
}

fn shared_root(root: &str) -> Option<&'static SharedRoot> {
    SHARED_ROOTS
        .iter()
        .find(|r| r.root == root || (r.root == ROOT_PREFIX && is_preset_root(root)))
}

/// The shared root, logical root, path and label of a path, if Bring over
/// handles it.
pub(crate) fn classify(
    path: &PortablePath,
) -> Option<(&'static SharedRoot, &str, &str, &'static str)> {
    let PortablePath::Logical { root, rel } = path else {
        return None;
    };
    let shared = shared_root(root)?;
    Some((shared, root, rel, (shared.kind)(rel)?))
}

/// What identifies a file's copy: its content hash, or for a preset file
/// too big to hash (it can't travel anyway), its size.
fn copy_hash(item: &Item) -> Option<ContentHash> {
    item.content_hash.clone().or_else(|| {
        let size = item
            .size
            .filter(|s| *s > MAX_FILE_BYTES && is_preset_file(item))?;
        Some(size_only_hash(size))
    })
}

/// The stand-in hash of a file known only by its size.
pub fn size_only_hash(size: u64) -> ContentHash {
    patchpilot_sync::content_hash(format!("size-only:{size}").as_bytes())
}

/// One file Primary Rig has that this machine lacks or has a different copy of.
/// Mirrored in `app/src/api.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileToBring {
    /// The app the file belongs to, for grouping: "Reaper".
    pub app: String,
    /// "Track template", "FX chain", "Script", "Theme", ...
    pub kind: String,
    /// Logical root, e.g. `REAPER_RESOURCES`.
    pub root: String,
    /// `/`-separated path below the root, e.g. `FXChains/Vocal Bus.RfxChain`.
    pub rel: String,
    pub hash: ContentHash,
    pub size: Option<u64>,
    /// True when this machine has a different file at that path (it is backed
    /// up before being replaced).
    pub replaces: bool,
    /// Over [`MAX_FILE_BYTES`], so it can't travel over the link.
    pub too_large: bool,
    /// Its product, to find it from the comparison's rows.
    pub product_id: Option<ProductId>,
    /// Other files in this list it needs beside it to work (decision #62),
    /// as `root/rel` keys: they are brought over with it.
    pub needs: Vec<String>,
    /// It, or a file it needs, can't travel, so it would arrive incomplete:
    /// it isn't brought over.
    pub incomplete: bool,
}

impl FileToBring {
    /// `root/rel`, as the app names files to bring.
    pub fn key(&self) -> String {
        format!("{}/{}", self.root, self.rel)
    }
}

/// A user-owned file this machine shares with its linked machines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedFile {
    pub hash: ContentHash,
    pub path: PathBuf,
    pub root: String,
    pub rel: String,
}

/// The bytes to upload for a shared file, as of now: `None` if it can't be
/// read or has changed since the scan (the next scan shares the new one).
pub fn read_shared(file: &SharedFile) -> Option<Vec<u8>> {
    let bytes = std::fs::read(&file.path).ok()?;
    let dir = file.path.ancestors().nth(file.rel.split('/').count())?;
    let bytes = (shared_root(&file.root)?.view)(dir, &file.rel, bytes);
    (patchpilot_sync::content_hash(&bytes) == file.hash).then_some(bytes)
}

/// The files on this machine others may bring over: every item in its latest
/// scan under a [`SHARED_ROOTS`] root, with a hash, that fits the relay.
pub fn shared_files(machine: &Manifest) -> Vec<SharedFile> {
    machine
        .items
        .iter()
        .filter(|i| i.size.is_some_and(|s| s <= MAX_FILE_BYTES))
        .filter_map(|i| {
            let (_, root, rel, _) = classify(&i.path)?;
            let path = machine.machine.path_map.resolve(&i.path)?;
            Some(SharedFile {
                hash: i.content_hash.clone()?,
                path: PathBuf::from(path),
                root: root.to_owned(),
                rel: rel.to_owned(),
            })
        })
        .collect()
}

/// What Primary Rig (narrowed by this machine's profile) has that `machine` lacks
/// or holds a different copy of, sorted by app, then path, each with the
/// files it needs. `machine` is `None` before this machine's first scan.
/// Another linked machine's latest scan, with the default profile, works as
/// `golden` too.
///
/// Presets come only from folders the profile keeps, only where this machine
/// has a place for them, and from the other OS only when both lay them out
/// the same way.
pub fn files_to_bring(
    golden: &Manifest,
    profile: &TargetProfile,
    machine: Option<&Manifest>,
) -> Vec<FileToBring> {
    let target = apply_profile(golden, profile);
    let os = machine.map(|m| m.machine.os);
    // Paths compare case-insensitively (NTFS and APFS both are). When both
    // Primary Rig slots have a file, prefer this OS's.
    let key = |root: &str, rel: &str| (root.to_owned(), rel.to_ascii_lowercase());
    // Preset folders the profile keeps, by root, with the folder's name.
    let preset_folders: BTreeMap<&str, &str> = target
        .items
        .iter()
        .filter(|i| presets::is_preset_item(i))
        .filter_map(|i| match &i.path {
            PortablePath::Logical { root, rel } if rel.is_empty() => {
                let name = target.product(i.product_id.as_ref()?)?.name.as_str();
                Some((root.as_str(), name))
            }
            _ => None,
        })
        .collect();
    let placeable = |root: &str| {
        machine.is_some_and(|m| m.machine.path_map.roots.contains_key(root))
            || os.is_none_or(|os| presets::is_vendor_root(root, os))
    };
    let mut wanted: BTreeMap<(String, String), (Os, &Item)> = BTreeMap::new();
    for item in &target.items {
        let Some((_, root, rel, _)) = classify(&item.path) else {
            continue;
        };
        if copy_hash(item).is_none() {
            continue;
        }
        if is_preset_file(item)
            && (!preset_folders.contains_key(root)
                || !placeable(root)
                || (!item.platform_portable && os.is_some_and(|os| os != item.platform)))
        {
            continue;
        }
        let k = key(root, rel);
        let better = match wanted.get(&k) {
            None => true,
            Some((have, _)) => Some(*have) != os && Some(item.platform) == os,
        };
        if better {
            wanted.insert(k, (item.platform, item));
        }
    }
    let here: BTreeMap<(String, String), ContentHash> = machine
        .into_iter()
        .flat_map(|m| &m.items)
        .filter_map(|i| {
            let (_, root, rel, _) = classify(&i.path)?;
            Some((key(root, rel), copy_hash(i)?))
        })
        .collect();

    let mut out: Vec<FileToBring> = wanted
        .into_iter()
        .filter_map(|(k, (_, item))| {
            let hash = copy_hash(item)?;
            let replaces = match here.get(&k) {
                Some(h) if *h == hash => return None,
                Some(_) => true,
                None => false,
            };
            let (shared, root, rel, kind) = classify(&item.path)?;
            // Presets group by their folder: "FabFilter presets".
            let kind = preset_folders.get(root).copied().unwrap_or(kind);
            Some(FileToBring {
                app: shared.app.to_owned(),
                kind: kind.to_owned(),
                root: root.to_owned(),
                rel: rel.to_owned(),
                too_large: item.size.is_some_and(|s| s > MAX_FILE_BYTES),
                size: item.size,
                hash,
                replaces,
                product_id: item.product_id.clone(),
                // Every file it needs, listed or not; narrowed below.
                needs: item
                    .needs_files
                    .iter()
                    .map(|n| format!("{root}/{n}"))
                    .collect(),
                incomplete: false,
            })
        })
        .collect();
    out.sort_by(|a, b| (&a.app, &a.rel).cmp(&(&b.app, &b.rel)));
    link(out)
}

/// Narrow each file's `needs` to files in the list (the rest are here
/// already), spelled as listed, and mark the files that can't arrive whole.
fn link(mut files: Vec<FileToBring>) -> Vec<FileToBring> {
    let listed: HashMap<String, String> = files
        .iter()
        .map(|f| (f.key().to_lowercase(), f.key()))
        .collect();
    for f in &mut files {
        let mut needs: Vec<String> = f
            .needs
            .iter()
            .filter_map(|n| listed.get(&n.to_lowercase()).cloned())
            .filter(|n| *n != f.key())
            .collect();
        needs.sort();
        needs.dedup();
        f.needs = needs;
    }
    let bad: BTreeSet<String> = files
        .iter()
        .filter(|f| f.too_large)
        .map(FileToBring::key)
        .collect();
    let blocked = blocked(&files, &bad);
    for f in &mut files {
        f.incomplete = f.too_large || blocked.contains(&f.key());
    }
    files
}

/// `keys` and every listed file they need, directly or through others.
pub fn with_needs<'a>(files: &'a [FileToBring], keys: &[String]) -> Vec<&'a FileToBring> {
    let by_key: HashMap<String, &FileToBring> = files.iter().map(|f| (f.key(), f)).collect();
    let all = patchpilot_parity::reaper_links::with_needs(keys.iter().map(String::as_str), |k| {
        by_key.get(k).map(|f| f.needs.as_slice())
    });
    files.iter().filter(|f| all.contains(&f.key())).collect()
}

/// Keys of the files that need one of `bad` (files that can't be had),
/// directly or through others, so they'd arrive without it. `bad` itself
/// isn't included.
pub fn blocked(files: &[FileToBring], bad: &BTreeSet<String>) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    loop {
        let before = out.len();
        for f in files {
            let key = f.key();
            if !bad.contains(&key)
                && !out.contains(&key)
                && f.needs.iter().any(|n| bad.contains(n) || out.contains(n))
            {
                out.insert(key);
            }
        }
        if out.len() == before {
            return out;
        }
    }
}

/// `files` without the ones this machine already has the same copy of, by
/// reading them now: its last scan may be older than the files (they were
/// written since), or a key map may hold the same shortcuts with this
/// machine's paths. `source` is the manifest the files come from.
pub fn drop_same_here(
    source: &Manifest,
    machine: Option<&Manifest>,
    files: Vec<FileToBring>,
) -> Vec<FileToBring> {
    let mut dirs: HashMap<String, Option<PathBuf>> = HashMap::new();
    let mut kept = Vec::new();
    for f in files {
        let dir = dirs
            .entry(f.root.clone())
            .or_insert_with(|| root_dir(machine, &f.root))
            .clone();
        let same = dir.is_some_and(|dir| same_here(source, &f, &dir));
        if !same {
            kept.push(f);
        }
    }
    link(kept)
}

fn same_here(source: &Manifest, f: &FileToBring, dir: &Path) -> bool {
    let path = f
        .rel
        .split('/')
        .fold(dir.to_owned(), |p, part| p.join(part));
    let Ok(bytes) = std::fs::read(&path) else {
        return false;
    };
    let Some(shared) = shared_root(&f.root) else {
        return false;
    };
    let hash = |b: &[u8]| patchpilot_sync::content_hash(b);
    if hash(&(shared.view)(dir, &f.rel, bytes.clone())) == f.hash || hash(&bytes) == f.hash {
        return true;
    }
    // A key map from a build before #62 has the source's own paths in it.
    if f.root != REAPER_RESOURCES || resource_kind(&f.rel) != Some(ResourceKind::KeyMap) {
        return false;
    }
    let (Some(from), Ok(text)) = (
        source.machine.path_map.roots.get(REAPER_RESOURCES),
        String::from_utf8(bytes),
    ) else {
        return false;
    };
    let theirs = patchpilot_parity::reaper::remap_key_map(
        &text,
        &dir.to_string_lossy(),
        this_os(),
        from,
        source.machine.os,
    );
    hash(theirs.as_bytes()) == f.hash
}

/// The hash of the copy of `rel` below `dir` (where `root` is here) as
/// others get it: the file itself, or for a settings file only its shareable
/// part, as scans and backups hash it. `None` when there's no file.
pub fn shared_hash(dir: &Path, root: &str, rel: &str) -> Option<ContentHash> {
    Some(patchpilot_sync::content_hash(&shared_copy(dir, root, rel)?))
}

/// The copy of `rel` below `dir` (where `root` is here) as others get it,
/// as [`shared_hash`] hashes it. `None` when there's no file.
pub fn shared_copy(dir: &Path, root: &str, rel: &str) -> Option<Vec<u8>> {
    let path = rel.split('/').fold(dir.to_owned(), |p, part| p.join(part));
    let bytes = std::fs::read(path).ok()?;
    Some(match shared_root(root) {
        Some(shared) => (shared.view)(dir, rel, bytes),
        None => bytes,
    })
}

/// Whether writing `incoming` (another machine's shared copy of `rel` below
/// `root`, at `dir` here) would change the file here, once merged as it would
/// be: a settings file shares only part of itself, and the same part can
/// look different from each machine (Places on drives only one has).
pub fn would_change(dir: &Path, root: &str, rel: &str, incoming: Vec<u8>) -> bool {
    let Some(shared) = shared_root(root) else {
        return true;
    };
    let path = rel.split('/').fold(dir.to_owned(), |p, part| p.join(part));
    let Ok(current) = std::fs::read(path) else {
        return true;
    };
    (shared.merge)(rel, Some(&current), incoming).is_ok_and(|merged| merged != current)
}

/// The DAW that must be closed before `rel` below `root` is written (CLAUDE.md
/// rule 5), if any.
pub fn needs_closed(root: &str, rel: &str) -> Option<Daw> {
    let shared = shared_root(root)?;
    shared.daw.or((shared.closed_daw)(rel))
}

/// Where `root` is on this machine: as its last scan mapped it, else the
/// root's default place.
pub fn root_dir(machine: Option<&Manifest>, root: &str) -> Option<PathBuf> {
    if let Some(dir) = machine.and_then(|m| m.machine.path_map.roots.get(root)) {
        return Some(PathBuf::from(dir));
    }
    if is_preset_root(root) {
        let os = Os::current()?;
        return presets::default_dir(root, os, &crate::preset_bases(os));
    }
    (shared_root(root)?.default_dir)()
}

/// Reaper's standard resource folder, if it exists.
fn default_reaper_dir() -> Option<PathBuf> {
    let os = Os::current()?;
    let appdata = std::env::var("APPDATA").unwrap_or_default();
    let home = crate::home_dir().unwrap_or_default();
    reaper_resource_dir(os, &appdata, &home).filter(|d| d.is_dir())
}

/// A file's `rel` below its root and its bytes, as [`write_files`] takes them.
pub type Fetched = (String, Vec<u8>);

/// What [`write_files`] did. Mirrored in `app/src/api.ts`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct WriteReport {
    pub written: usize,
    /// Files that were already the same here, so weren't written.
    pub same: usize,
    /// Files that were replaced; their old bytes are in the restore point.
    pub backed_up: usize,
    /// Files removed (the old name of a file renamed on another machine);
    /// their bytes are in the restore point.
    pub removed: usize,
    /// The restore point History can undo, if anything was written.
    pub restore_point: Option<i64>,
}

/// A file from the Primary Rig made right for this machine before it is
/// written: script paths in Reaper's key map that point into Primary Rig's
/// resource folder are moved to this machine's (`dir`), and on a Mac that
/// keeps them so, shortcuts macOS keeps for itself move to Control. Anything
/// else is returned as it came.
pub fn adapt_for_here(
    golden: &Manifest,
    root: &str,
    rel: &str,
    dir: &Path,
    os: Os,
    bytes: Vec<u8>,
) -> Vec<u8> {
    use patchpilot_parity::reaper::{KEY_MAP_ROOT, remap_key_map};
    if root != REAPER_RESOURCES || resource_kind(rel) != Some(ResourceKind::KeyMap) {
        return bytes;
    }
    let text = match String::from_utf8(bytes) {
        Ok(t) => t,
        // Not UTF-8: leave it alone rather than mangle it.
        Err(e) => return e.into_bytes(),
    };
    let here = dir.to_string_lossy();
    // Shared portably since #62; older builds shared their own paths.
    let text = remap_key_map(&text, KEY_MAP_ROOT, Os::Macos, &here, os);
    let text = match golden.machine.path_map.roots.get(REAPER_RESOURCES) {
        Some(from) => remap_key_map(&text, from, golden.machine.os, &here, os),
        None => text,
    };
    // Shortcuts macOS keeps for itself move to Control (decision #81).
    if os == Os::Macos && patchpilot_parity::reaper_keys::mac_form() {
        patchpilot_parity::reaper_keys::for_mac(&text)
    } else {
        text
    }
    .into_bytes()
}

/// Write files (`rel` below `root`, which is at `dir` here, and their bytes)
/// through [`crate::restore::apply_changes`]: every file it replaces is saved
/// first, each is written to a temp file beside it and renamed into place,
/// and History can undo the lot. Every path and the root's guard are checked
/// first, and nothing is written if any fails.
#[allow(clippy::too_many_arguments)]
pub fn write_files(
    store: &mut Store,
    blobs: &Path,
    machine_id: &str,
    label: &str,
    root: &str,
    dir: &Path,
    files: &[(String, Vec<u8>)],
    now: i64,
) -> Result<WriteReport, String> {
    write_changes(store, blobs, machine_id, label, root, dir, files, &[], now)
}

/// [`write_files`], also removing the files at `remove` (`rel`s below `root`)
/// in the same restore point, so undoing it puts them back. Folders left
/// empty by a removal are removed too, except the root's own top folders.
#[allow(clippy::too_many_arguments)]
pub fn write_changes(
    store: &mut Store,
    blobs: &Path,
    machine_id: &str,
    label: &str,
    root: &str,
    dir: &Path,
    files: &[(String, Vec<u8>)],
    remove: &[String],
    now: i64,
) -> Result<WriteReport, String> {
    let shared = shared_root(root).ok_or_else(|| format!("{root}: not a shared folder"))?;
    (shared.guard)()?;
    let mut changes = Vec::new();
    let mut backed_up = 0;
    let mut same = 0;
    let mut daw = None;
    for (rel, bytes) in files {
        if (shared.kind)(rel).is_none() {
            return Err(format!("{rel}: not a file {} can receive", shared.app));
        }
        let target = rel.split('/').fold(dir.to_owned(), |p, part| p.join(part));
        let current = match std::fs::read(&target) {
            Ok(old) => Some(old),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(format!("{}: {e}", target.display())),
        };
        let bytes = (shared.merge)(rel, current.as_deref(), bytes.clone())?;
        match current {
            Some(old) if old == bytes => {
                same += 1;
                continue;
            }
            Some(_) => backed_up += 1,
            None => {}
        }
        daw = daw.or((shared.closed_daw)(rel));
        changes.push(FileChange {
            path: target,
            contents: Some(bytes),
        });
    }
    let mut removed = Vec::new();
    for rel in remove {
        if (shared.kind)(rel).is_none() {
            return Err(format!("{rel}: not a file {} can remove", shared.app));
        }
        let target = rel.split('/').fold(dir.to_owned(), |p, part| p.join(part));
        if !target.is_file() {
            continue;
        }
        daw = daw.or((shared.closed_daw)(rel));
        removed.push((rel.clone(), target.clone()));
        changes.push(FileChange {
            path: target,
            contents: None,
        });
    }
    let req = ChangeSet {
        machine_id: machine_id.to_owned(),
        label: label.to_owned(),
        // Reaper's templates, scripts and themes aren't rewritten on exit, so
        // Reaper may stay open, but key maps and menus need it closed;
        // Ableton's root sets `Daw::Ableton`.
        daw: shared.daw.or(daw),
        changes,
        now,
    };
    let point = apply_changes(store, blobs, &req).map_err(|e| e.to_string())?;
    for (rel, target) in &removed {
        // Keep the root's own folders (`TrackTemplates`, `Presets`).
        let top = dir.join(rel.split('/').next().unwrap_or(rel));
        crate::backup::remove_empty_parents(target, &top);
    }
    let removed = removed
        .iter()
        .filter(|(_, t)| point.changed.contains(t))
        .count();
    Ok(WriteReport {
        written: point.changed.len() - removed,
        same,
        backed_up,
        removed,
        restore_point: point.id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use patchpilot_core::{Confidence, ItemKind, MachineInfo, PathMap};

    fn hash(s: &str) -> ContentHash {
        patchpilot_sync::content_hash(s.as_bytes())
    }

    fn template(os: Os, rel: &str, body: &str) -> Item {
        Item {
            kind: ItemKind::DawAsset,
            product_id: None,
            format: None,
            platform: os,
            arch: None,
            version: None,
            path: PortablePath::logical(REAPER_RESOURCES, rel),
            content_hash: Some(hash(body)),
            size: Some(body.len() as u64),
            mtime: None,
            platform_portable: true,
            confidence: Confidence::High,
            requires: Vec::new(),
            needs_files: Vec::new(),
        }
    }

    fn manifest(os: Os, dir: &str, items: Vec<Item>) -> Manifest {
        let mut m = Manifest::new(
            MachineInfo {
                id: "m1".into(),
                name: "Machine".into(),
                os,
                os_version: None,
                arch: patchpilot_core::Arch::Aarch64,
                path_map: PathMap::new(os).with_root(REAPER_RESOURCES, dir),
            },
            0,
        );
        m.items = items;
        m
    }

    #[test]
    fn lists_what_golden_has_that_this_machine_lacks_or_has_differently() {
        let golden = manifest(
            Os::Windows,
            r"C:\Users\a\AppData\Roaming\REAPER",
            vec![
                template(Os::Windows, "FXChains/Vocal Bus.RfxChain", "vocal"),
                template(
                    Os::Windows,
                    "TrackTemplates/Drums.RTrackTemplate",
                    "drums v2",
                ),
                template(Os::Windows, "ProjectTemplates/Mix.RPP", "mix"),
                template(Os::Windows, "ColorThemes/Imperial.ReaperThemeZip", "theme"),
            ],
        );
        let mac = manifest(
            Os::Macos,
            "/Users/a/Library/Application Support/REAPER",
            vec![
                template(Os::Macos, "TrackTemplates/Drums.RTrackTemplate", "drums v1"),
                template(Os::Macos, "projecttemplates/mix.rpp", "mix"),
            ],
        );
        let list = files_to_bring(&golden, &TargetProfile::default(), Some(&mac));
        let got: Vec<_> = list
            .iter()
            .map(|f| (f.app.as_str(), f.kind.as_str(), f.rel.as_str(), f.replaces))
            .collect();
        assert_eq!(
            got,
            [
                (
                    "Reaper",
                    "Theme",
                    "ColorThemes/Imperial.ReaperThemeZip",
                    false
                ),
                ("Reaper", "FX chain", "FXChains/Vocal Bus.RfxChain", false),
                (
                    "Reaper",
                    "Track template",
                    "TrackTemplates/Drums.RTrackTemplate",
                    true
                ),
            ]
        );
        assert_eq!(list[1].hash, hash("vocal"));
        // Nothing to bring onto the machine Primary Rig came from.
        assert!(files_to_bring(&golden, &TargetProfile::default(), Some(&golden)).is_empty());
        // Before a first scan: everything.
        assert_eq!(
            files_to_bring(&golden, &TargetProfile::default(), None).len(),
            4
        );
    }

    #[test]
    fn collection_names_merge_into_this_machines_library_cfg() {
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/parity/ableton/prefs-mac/Live 12.1.5/Library.cfg");
        let tmp = tempfile::tempdir().unwrap();
        let prefs = tmp.path().join("Live 12.1.5");
        std::fs::create_dir_all(&prefs).unwrap();
        std::fs::copy(&fixture, prefs.join("Library.cfg")).unwrap();

        // What this machine shares is the names, hidden sidebar items and the
        // Places in shared folders (none here), checked against the scan's hash.
        let names = b"1\tFaves\n2\tSynths & Keys\n3\t\n4\t\n5\t\n6\t\n7\t\nhidden\nplaces\n";
        let file = SharedFile {
            hash: patchpilot_sync::content_hash(names),
            path: prefs.join("Library.cfg"),
            root: ABLETON_PREFS.into(),
            rel: "Library.cfg".into(),
        };
        assert_eq!(read_shared(&file).unwrap(), names);

        let mut store = Store::open_in_memory().unwrap();
        let result = write_files(
            &mut store,
            &tmp.path().join("blobs"),
            "m1",
            "Brought over 1 Ableton file",
            ABLETON_PREFS,
            &prefs,
            &[("Library.cfg".into(), b"1\tDrums\n2\tKeys\n".to_vec())],
            1,
        );
        // Only possible while Live is closed, as on CI.
        if crate::apps::ableton_live_running() {
            assert!(result.is_err());
            return;
        }
        assert_eq!(result.unwrap().backed_up, 1);
        let cfg = std::fs::read_to_string(prefs.join("Library.cfg")).unwrap();
        assert!(cfg.contains(r#"Uri="color:colors=1" DisplayName="Drums""#));
        assert!(cfg.contains(r#"Uri="color:colors=2" DisplayName="Keys""#));
        assert!(cfg.contains(r#"<ProjectPath Value="/Users/alice/Music/Ableton/User Library" />"#));
    }

    #[test]
    fn ableton_settings_are_listed_under_ableton() {
        let at = |root: &str, rel: &str, body: &str| {
            let mut item = template(Os::Windows, rel, body);
            item.path = PortablePath::logical(root, rel);
            item
        };
        let golden = manifest(
            Os::Windows,
            r"C:\Users\a\AppData\Roaming\REAPER",
            vec![
                at(ABLETON_USER_LIBRARY, "Templates/Default.als", "set"),
                at(
                    ABLETON_USER_LIBRARY,
                    "Ableton Folder Info/12/Collections.xmp",
                    "tags",
                ),
                at(ABLETON_USER_LIBRARY, "Samples/Imported/kick.wav", "audio"),
                at(ABLETON_PREFS, "Options.txt", "-ShowDeviceSlots"),
                at(ABLETON_PREFS, "Preferences.cfg", "binary"),
            ],
        );
        let list = files_to_bring(&golden, &TargetProfile::default(), None);
        let got: Vec<_> = list
            .iter()
            .map(|f| (f.app.as_str(), f.kind.as_str(), f.rel.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                (
                    "Ableton",
                    "Collections",
                    "Ableton Folder Info/12/Collections.xmp"
                ),
                ("Ableton", "Setting", "Options.txt"),
                ("Ableton", "Template", "Templates/Default.als"),
            ]
        );
    }

    #[test]
    fn never_writes_ableton_preferences_cfg_or_samples() {
        let tmp = tempfile::tempdir().unwrap();
        for (root, rel) in [
            (ABLETON_PREFS, "Preferences.cfg"),
            (ABLETON_PREFS, "Log.txt"),
            (ABLETON_USER_LIBRARY, "Samples/Imported/kick.wav"),
        ] {
            let dir = tmp.path().join(root);
            let mut store = Store::open_in_memory().unwrap();
            let result = write_files(
                &mut store,
                &tmp.path().join("b"),
                "m1",
                "x",
                root,
                &dir,
                &[(rel.into(), b"x".to_vec())],
                1,
            );
            assert!(result.is_err(), "{root}/{rel}");
            assert!(!dir.join(rel).exists());
        }
    }

    #[test]
    fn shares_only_templates_that_resolve_and_fit() {
        let dir = "/Users/a/Library/Application Support/REAPER";
        let mut big = template(Os::Macos, "FXChains/Huge.RfxChain", "x");
        big.size = Some(MAX_FILE_BYTES + 1);
        let mut outside = template(Os::Macos, "reaper.ini", "ini");
        outside.path = PortablePath::logical(REAPER_RESOURCES, "reaper.ini");
        let m = manifest(
            Os::Macos,
            dir,
            vec![
                template(Os::Macos, "FXChains/Vocal Bus.RfxChain", "vocal"),
                template(Os::Macos, "Scripts/Tools/Nudge.lua", "nudge"),
                big,
                outside,
            ],
        );
        assert_eq!(
            shared_files(&m),
            [
                SharedFile {
                    hash: hash("vocal"),
                    path: PathBuf::from(format!("{dir}/FXChains/Vocal Bus.RfxChain")),
                    root: REAPER_RESOURCES.into(),
                    rel: "FXChains/Vocal Bus.RfxChain".into(),
                },
                SharedFile {
                    hash: hash("nudge"),
                    path: PathBuf::from(format!("{dir}/Scripts/Tools/Nudge.lua")),
                    root: REAPER_RESOURCES.into(),
                    rel: "Scripts/Tools/Nudge.lua".into(),
                },
            ]
        );
    }

    #[test]
    fn writes_new_files_and_backs_up_replaced_ones() {
        let tmp = tempfile::tempdir().unwrap();
        let reaper = tmp.path().join("REAPER");
        let blobs = tmp.path().join("PatchPilot").join("blobs");
        let mut store = Store::open_in_memory().unwrap();
        std::fs::create_dir_all(reaper.join("TrackTemplates")).unwrap();
        std::fs::write(reaper.join("TrackTemplates/Drums.RTrackTemplate"), "old").unwrap();
        std::fs::write(reaper.join("TrackTemplates/Same.RTrackTemplate"), "same").unwrap();

        let report = write_files(
            &mut store,
            &blobs,
            "m1",
            "Brought over 2 Reaper files",
            REAPER_RESOURCES,
            &reaper,
            &[
                ("FXChains/Mix/Vocal Bus.RfxChain".into(), b"vocal".to_vec()),
                (
                    "TrackTemplates/Drums.RTrackTemplate".into(),
                    b"new".to_vec(),
                ),
                (
                    "TrackTemplates/Same.RTrackTemplate".into(),
                    b"same".to_vec(),
                ),
            ],
            1,
        )
        .unwrap();
        assert_eq!((report.written, report.backed_up), (2, 1));
        let read = |p: &Path| std::fs::read_to_string(p).unwrap();
        assert_eq!(
            read(&reaper.join("FXChains/Mix/Vocal Bus.RfxChain")),
            "vocal"
        );
        assert_eq!(
            read(&reaper.join("TrackTemplates/Drums.RTrackTemplate")),
            "new"
        );
        // History can undo it: the old template comes back, the new chain goes.
        crate::restore::undo(&mut store, &blobs, report.restore_point.unwrap(), 2).unwrap();
        assert_eq!(
            read(&reaper.join("TrackTemplates/Drums.RTrackTemplate")),
            "old"
        );
        assert!(!reaper.join("FXChains/Mix/Vocal Bus.RfxChain").exists());
        // No temporary files left beside the templates.
        let names: Vec<_> = std::fs::read_dir(reaper.join("TrackTemplates"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(names.iter().all(|n| !n.contains("patchpilot")), "{names:?}");
    }

    #[test]
    fn refuses_paths_outside_the_template_folders_before_writing_anything() {
        let tmp = tempfile::tempdir().unwrap();
        let reaper = tmp.path().join("REAPER");
        let mut store = Store::open_in_memory().unwrap();
        let err = write_files(
            &mut store,
            &tmp.path().join("b"),
            "m1",
            "x",
            REAPER_RESOURCES,
            &reaper,
            &[
                ("FXChains/ok.RfxChain".into(), b"ok".to_vec()),
                ("Scripts/../reaper.ini".into(), b"x".to_vec()),
            ],
            1,
        )
        .unwrap_err();
        assert!(err.contains("reaper.ini"));
        assert!(!reaper.exists());
    }

    #[test]
    fn presets_come_from_kept_folders_and_the_same_layout() {
        use patchpilot_core::{Product, ProductId};
        // A preset folder item and its files, as the scan lists them.
        let folder = |os: Os, id: &str, root: &str, name: &str, files: &[(&str, &str, bool)]| {
            let mut items = vec![Item {
                kind: ItemKind::Folder,
                product_id: Some(ProductId(format!("presets/{id}"))),
                path: PortablePath::logical(root, ""),
                ..template(os, "x", "folder")
            }];
            for (rel, body, portable) in files {
                items.push(Item {
                    path: PortablePath::logical(root, *rel),
                    platform_portable: *portable,
                    ..template(os, rel, body)
                });
            }
            let product = Product {
                canonical_id: ProductId(format!("presets/{id}")),
                vendor: String::new(),
                name: name.into(),
            };
            (product, items)
        };
        let mut golden = manifest(Os::Windows, r"C:\R", Vec::new());
        for (p, items) in [
            folder(
                Os::Windows,
                "fabfilter",
                "PRESETS_FABFILTER",
                "FabFilter presets",
                &[
                    ("Pro-Q 4/Air.ffp", "air", true),
                    ("Pro-C 2/Glue.ffp", "glue", true),
                ],
            ),
            folder(
                Os::Windows,
                "u-he",
                "PRESETS_U_HE",
                "u-he presets",
                &[("Diva.data/Presets/Diva/Bass.h2p", "bass", false)],
            ),
            folder(
                Os::Windows,
                "standard-acme",
                "PRESETS_STANDARD_ACME",
                "Acme presets",
                &[("Verb/Hall.vstpreset", "hall", true)],
            ),
        ] {
            golden.products.push(p);
            golden.items.extend(items);
        }
        let mut big = golden.items[1].clone();
        big.path = PortablePath::logical("PRESETS_FABFILTER", "Pro-Q 4/Huge.ffp");
        big.content_hash = None;
        big.size = Some(MAX_FILE_BYTES * 3);
        golden.items.push(big);

        let mut mac = manifest(Os::Macos, "/R", Vec::new());
        mac.items.push(Item {
            path: PortablePath::logical("PRESETS_FABFILTER", "pro-c 2/glue.ffp"),
            ..template(Os::Macos, "x", "glue")
        });
        let list = files_to_bring(&golden, &TargetProfile::default(), Some(&mac));
        let got: Vec<_> = list
            .iter()
            .map(|f| {
                (
                    f.app.as_str(),
                    f.kind.as_str(),
                    f.root.as_str(),
                    f.rel.as_str(),
                    f.too_large,
                )
            })
            .collect();
        // Glue is here already; u-he lays its presets out differently per OS;
        // Acme's standard folder isn't on this Mac yet, so there's no place for it.
        assert_eq!(
            got,
            [
                (
                    "Presets",
                    "FabFilter presets",
                    "PRESETS_FABFILTER",
                    "Pro-Q 4/Air.ffp",
                    false
                ),
                (
                    "Presets",
                    "FabFilter presets",
                    "PRESETS_FABFILTER",
                    "Pro-Q 4/Huge.ffp",
                    true
                ),
            ]
        );
        // Once the Mac has Acme's folder, its VST3 preset can come over.
        mac.machine.path_map.roots.insert(
            "PRESETS_STANDARD_ACME".into(),
            "/Users/a/Library/Audio/Presets/Acme".into(),
        );
        let list = files_to_bring(&golden, &TargetProfile::default(), Some(&mac));
        assert!(
            list.iter()
                .any(|f| f.rel == "Verb/Hall.vstpreset" && f.kind == "Acme presets")
        );
        // The same big file (by size) on the Mac isn't listed again.
        let mut big_here = golden.items.last().unwrap().clone();
        big_here.platform = Os::Macos;
        mac.items.push(big_here);
        let list = files_to_bring(&golden, &TargetProfile::default(), Some(&mac));
        assert!(!list.iter().any(|f| f.too_large));
        // Big files never go up to the relay.
        assert!(
            shared_files(&golden)
                .iter()
                .all(|f| !f.rel.contains("Huge"))
        );
    }

    #[test]
    fn presets_are_written_and_unsafe_paths_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("FabFilter").join("Presets");
        let mut store = Store::open_in_memory().unwrap();
        let blobs = tmp.path().join("blobs");
        for bad in ["../Air.ffp", "Pro-Q 4/plugin.dll", ".hidden/x.ffp"] {
            let err = write_files(
                &mut store,
                &blobs,
                "m1",
                "x",
                "PRESETS_FABFILTER",
                &dir,
                &[(bad.into(), b"x".to_vec())],
                1,
            );
            assert!(err.is_err(), "{bad}");
        }
        let report = write_files(
            &mut store,
            &blobs,
            "m1",
            "Brought over 1 preset",
            "PRESETS_FABFILTER",
            &dir,
            &[("Pro-Q 4/Air.ffp".into(), b"air".to_vec())],
            1,
        )
        .unwrap();
        assert_eq!(report.written, 1);
        assert_eq!(
            std::fs::read(dir.join("Pro-Q 4").join("Air.ffp")).unwrap(),
            b"air"
        );
        assert!(report.restore_point.is_some());
    }

    #[test]
    fn key_maps_and_menus_need_reaper_closed() {
        let reaper = &SHARED_ROOTS[0];
        assert_eq!((reaper.closed_daw)("reaper-kb.ini"), Some(Daw::Reaper));
        assert_eq!((reaper.closed_daw)("reaper-menu.ini"), Some(Daw::Reaper));
        assert_eq!((reaper.closed_daw)("Scripts/a.lua"), None);
        assert_eq!((reaper.kind)("reaper-kb.ini"), Some("Key map"));
        assert_eq!((reaper.kind)("reaper.ini"), None);
    }

    #[test]
    fn key_map_script_paths_are_moved_to_this_machine() {
        let golden = manifest(
            Os::Windows,
            r"C:\Users\j\AppData\Roaming\REAPER",
            Vec::new(),
        );
        let kb = b"SCR 4 0 RS1 \"Custom: a.lua\" \"C:\\Users\\j\\AppData\\Roaming\\REAPER\\Scripts\\a.lua\"\n".to_vec();
        let here = Path::new("/Users/j/Library/Application Support/REAPER");
        let out = adapt_for_here(
            &golden,
            REAPER_RESOURCES,
            "reaper-kb.ini",
            here,
            Os::Macos,
            kb.clone(),
        );
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "SCR 4 0 RS1 \"Custom: a.lua\" \"/Users/j/Library/Application Support/REAPER/Scripts/a.lua\"\n"
        );
        let menus = adapt_for_here(
            &golden,
            REAPER_RESOURCES,
            "reaper-menu.ini",
            here,
            Os::Macos,
            kb.clone(),
        );
        assert_eq!(menus, kb, "only the key map is rewritten");
    }

    #[test]
    fn files_come_with_what_they_need_or_not_at_all() {
        let needing = |rel: &str, body: &str, needs: &[&str]| {
            let mut i = template(Os::Windows, rel, body);
            i.needs_files = needs.iter().map(|s| s.to_string()).collect();
            i
        };
        let mut big = needing("Scripts/Big/main.lua", "big", &["Scripts/Big/samples.dat"]);
        big.size = Some(10);
        let mut data = template(Os::Windows, "Scripts/Big/samples.dat", "dat");
        data.size = Some(MAX_FILE_BYTES + 1);
        let golden = manifest(
            Os::Windows,
            r"C:\Users\a\AppData\Roaming\REAPER",
            vec![
                needing(
                    "Scripts/Tool/main.lua",
                    "main",
                    &["Scripts/Tool/lib/gui.lua", "Scripts/Lib/core.lua"],
                ),
                needing("Scripts/Tool/lib/gui.lua", "gui", &[]),
                needing("Scripts/Lib/core.lua", "core", &[]),
                needing("Scripts/Other.lua", "other", &[]),
                big,
                data,
            ],
        );
        // This machine already has the shared library.
        let mac = manifest(
            Os::Macos,
            "/Users/a/Library/Application Support/REAPER",
            vec![template(Os::Macos, "Scripts/Lib/core.lua", "core")],
        );
        let list = files_to_bring(&golden, &TargetProfile::default(), Some(&mac));
        let main = list
            .iter()
            .find(|f| f.rel == "Scripts/Tool/main.lua")
            .unwrap();
        assert_eq!(main.needs, ["REAPER_RESOURCES/Scripts/Tool/lib/gui.lua"]);
        assert!(!main.incomplete);
        assert_eq!(main.product_id, golden.items[0].product_id);
        let big = list
            .iter()
            .find(|f| f.rel == "Scripts/Big/main.lua")
            .unwrap();
        assert!(big.incomplete, "needs a file too large to send");

        let picked = with_needs(&list, &["REAPER_RESOURCES/Scripts/Tool/main.lua".into()]);
        let rels: Vec<_> = picked.iter().map(|f| f.rel.as_str()).collect();
        assert_eq!(rels, ["Scripts/Tool/lib/gui.lua", "Scripts/Tool/main.lua"]);

        // If the library can't be had, the script stays behind too.
        let bad = BTreeSet::from(["REAPER_RESOURCES/Scripts/Tool/lib/gui.lua".to_owned()]);
        assert_eq!(
            blocked(&list, &bad),
            BTreeSet::from(["REAPER_RESOURCES/Scripts/Tool/main.lua".to_owned()])
        );
    }

    #[test]
    fn files_already_here_are_dropped_even_before_a_rescan() {
        let tmp = tempfile::tempdir().unwrap();
        let here = tmp.path().join("REAPER");
        std::fs::create_dir_all(here.join("Scripts")).unwrap();
        std::fs::write(here.join("Scripts/a.lua"), "same").unwrap();
        let win = r"C:\Users\j\AppData\Roaming\REAPER";
        // A key map from a Primary Rig scanned before #62, with its own paths.
        let kb = format!("SCR 4 0 RS1 \"Custom: a.lua\" \"{win}\\Scripts\\a.lua\"\n");
        let os = this_os();
        let sep = os.separator();
        std::fs::write(
            here.join("reaper-kb.ini"),
            format!(
                "SCR 4 0 RS1 \"Custom: a.lua\" \"{}{sep}Scripts{sep}a.lua\"\n",
                here.display()
            ),
        )
        .unwrap();
        let mut kb_item = template(Os::Windows, "reaper-kb.ini", &kb);
        kb_item.path = PortablePath::logical(REAPER_RESOURCES, "reaper-kb.ini");
        let golden = manifest(
            Os::Windows,
            win,
            vec![
                template(Os::Windows, "Scripts/a.lua", "same"),
                template(Os::Windows, "Scripts/b.lua", "new"),
                kb_item,
            ],
        );
        // This machine's last scan predates the files.
        let mac = manifest(os, &here.to_string_lossy(), Vec::new());
        let list = files_to_bring(&golden, &TargetProfile::default(), Some(&mac));
        assert_eq!(list.len(), 3);
        let left = drop_same_here(&golden, Some(&mac), list);
        let rels: Vec<_> = left.iter().map(|f| f.rel.as_str()).collect();
        assert_eq!(rels, ["Scripts/b.lua"]);
    }

    #[test]
    fn key_maps_travel_portably() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("REAPER");
        std::fs::create_dir_all(&dir).unwrap();
        let os = this_os();
        let sep = os.separator();
        let kb = format!(
            "SCR 4 0 RS1 \"Custom: a.lua\" \"{}{sep}Scripts{sep}a.lua\"\n",
            dir.display()
        );
        std::fs::write(dir.join("reaper-kb.ini"), &kb).unwrap();
        let portable = "SCR 4 0 RS1 \"Custom: a.lua\" \"<REAPER_RESOURCES>/Scripts/a.lua\"\n";
        let file = SharedFile {
            hash: hash(portable),
            path: dir.join("reaper-kb.ini"),
            root: REAPER_RESOURCES.into(),
            rel: "reaper-kb.ini".into(),
        };
        assert_eq!(read_shared(&file).unwrap(), portable.as_bytes());
        // And arrive with this machine's folder.
        let golden = manifest(Os::Windows, r"C:\R", Vec::new());
        let here = Path::new("/Users/j/REAPER");
        let out = adapt_for_here(
            &golden,
            REAPER_RESOURCES,
            "reaper-kb.ini",
            here,
            Os::Macos,
            portable.as_bytes().to_vec(),
        );
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "SCR 4 0 RS1 \"Custom: a.lua\" \"/Users/j/REAPER/Scripts/a.lua\"\n"
        );
    }
}
