//! Duplicate presets (decision #66): the same preset saved in several
//! folders on this machine, found by content hash, and cleaned up by
//! removing the extra copies through a restore point, so History can put
//! them back.
//!
//! Only preset-like files count (plug-in presets, Reaper FX chains and track
//! templates, Ableton device presets and racks), and only file metadata is
//! read until two files have the same size; then just those are hashed.
//! Where a copy lives decides which one to keep: a plug-in's own preset
//! folder or a DAW's folder beats Documents or Music, which beat Desktop
//! and Downloads.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use patchpilot_core::Os;
use patchpilot_store::Store;
use serde::{Deserialize, Serialize};

use crate::restore::{ChangeSet, Daw, FileChange, apply_changes, daw_running};

/// Extensions (lowercase) of files that are presets, FX chains or templates.
/// Projects (`.rpp`, `.als`) are left out: copies of those are usually
/// backups on purpose.
pub const PRESET_EXTENSIONS: &[&str] = &[
    // Plug-in formats and hosts.
    "fxp",
    "fxb",
    "vstpreset",
    "aupreset",
    "nksf",
    "nksfx",
    // Vendors.
    "ffp",         // FabFilter
    "h2p",         // u-he
    "vital",       // Vital
    "vitalbank",   // Vital
    "serumpreset", // Serum 2
    "nmsv",        // Massive
    "phaseplant",  // Kilohearts Phase Plant
    "prt_omn",     // Omnisphere
    // Reaper.
    "rfxchain",
    "rtracktemplate",
    // Ableton.
    "adv",
    "adg",
];

/// Never walk more entries than this in one place.
const MAX_ENTRIES: usize = 300_000;
/// How deep below a loose place (Documents, Downloads) to look.
const LOOSE_DEPTH: usize = 8;
/// Bigger files aren't presets, and restore points don't keep them.
const MAX_SIZE: u64 = 64 * 1024 * 1024;

/// How a place ranks when choosing the copy to keep.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaceKind {
    /// A plug-in's own or the standard preset folder.
    Standard,
    /// A folder a DAW reads presets or templates from.
    Daw,
    /// Documents, Music: where people keep things on purpose.
    Personal,
    /// Desktop, Downloads: where copies land by accident.
    Loose,
}

/// A folder to look in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    pub path: PathBuf,
    pub kind: PlaceKind,
    /// For people: "FabFilter presets", "Reaper FX chains", "Downloads".
    pub label: String,
    /// Removing files here waits until this DAW is closed.
    pub daw: Option<Daw>,
}

impl Place {
    fn new(path: PathBuf, kind: PlaceKind, label: impl Into<String>) -> Self {
        Self {
            path,
            kind,
            label: label.into(),
            daw: None,
        }
    }
}

/// One copy of a duplicated preset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Copy {
    pub path: String,
    /// The place it was found in, e.g. "Serum presets" or "Downloads".
    pub place: String,
    pub kind: PlaceKind,
    pub mtime: Option<i64>,
}

/// Files with the same contents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DuplicateGroup {
    /// BLAKE3 of the contents.
    pub hash: String,
    /// File name of the suggested copy.
    pub name: String,
    pub size: u64,
    /// Suggested copy to keep first, then the rest by rank.
    pub copies: Vec<Copy>,
}

/// Everything found.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Duplicates {
    /// Largest waste first.
    pub groups: Vec<DuplicateGroup>,
    /// Preset files looked at.
    pub files: usize,
    /// Labels of the places looked in.
    pub places: Vec<String>,
}

/// Where to look on this machine: preset folders, Reaper's FX chains and
/// track templates, Ableton's User Library, then Documents, Music, Desktop
/// and Downloads. Only places that exist.
pub fn standard_places() -> Vec<Place> {
    let Some(os) = Os::current() else {
        return Vec::new();
    };
    let mut places = Vec::new();
    for f in crate::preset_folders(os) {
        places.push(Place::new(f.path, PlaceKind::Standard, f.product.name));
    }
    if os == Os::Macos
        && let Some(home) = crate::home_dir()
    {
        // Loose files right in the standard AU/VST3 preset folder.
        let p = Path::new(&home).join("Library/Audio/Presets");
        places.push(Place::new(p, PlaceKind::Standard, "Audio presets"));
    }
    if os == Os::Windows
        && let Some(home) = crate::home_dir()
    {
        let p = Path::new(&home).join("Documents").join("VST3 Presets");
        places.push(Place::new(p, PlaceKind::Standard, "VST3 presets"));
    }
    if let Some(dir) = crate::standard_reaper_dir() {
        for (sub, label) in [
            ("FXChains", "Reaper FX chains"),
            ("TrackTemplates", "Reaper track templates"),
        ] {
            places.push(Place {
                daw: Some(Daw::Reaper),
                ..Place::new(dir.join(sub), PlaceKind::Daw, label)
            });
        }
    }
    if let Some(lib) = crate::standard_ableton_dirs().and_then(|d| d.user_library) {
        places.push(Place {
            daw: Some(Daw::Ableton),
            ..Place::new(lib, PlaceKind::Daw, "Ableton User Library")
        });
    }
    if let Some(home) = crate::home_dir() {
        let home = Path::new(&home);
        for (sub, kind) in [
            ("Documents", PlaceKind::Personal),
            ("Music", PlaceKind::Personal),
            ("Desktop", PlaceKind::Loose),
            ("Downloads", PlaceKind::Loose),
        ] {
            places.push(Place::new(home.join(sub), kind, sub));
        }
    }
    places.retain(|p| p.path.is_dir());
    places
}

struct Found {
    path: PathBuf,
    place: usize,
    size: u64,
    mtime: Option<i64>,
}

/// Find presets with the same contents in `places`. A file is counted once,
/// for the first place that holds it, so list the most specific first.
pub fn find_duplicates(places: &[Place]) -> Duplicates {
    let mut seen = HashSet::new();
    let mut by_size: BTreeMap<u64, Vec<Found>> = BTreeMap::new();
    let mut files = 0;
    for (i, place) in places.iter().enumerate() {
        let depth = match place.kind {
            PlaceKind::Personal | PlaceKind::Loose => LOOSE_DEPTH,
            _ => usize::MAX,
        };
        let walk = walkdir::WalkDir::new(&place.path)
            .follow_links(false)
            .max_depth(depth)
            .into_iter()
            .filter_entry(|e| e.depth() == 0 || !skip_dir(e))
            .take(MAX_ENTRIES)
            .flatten();
        for e in walk {
            if !e.file_type().is_file() || !is_preset(e.path()) {
                continue;
            }
            if !seen.insert(e.path().to_path_buf()) {
                continue;
            }
            let Ok(meta) = e.metadata() else { continue };
            files += 1;
            if meta.len() == 0 || meta.len() > MAX_SIZE {
                continue;
            }
            by_size.entry(meta.len()).or_default().push(Found {
                path: e.into_path(),
                place: i,
                size: meta.len(),
                mtime: mtime(&meta),
            });
        }
    }

    let mut groups = Vec::new();
    for (_, same_size) in by_size.into_iter().filter(|(_, v)| v.len() > 1) {
        let mut by_hash: BTreeMap<String, Vec<Found>> = BTreeMap::new();
        for f in same_size {
            if let Some(h) = hash_file(&f.path) {
                by_hash.entry(h).or_default().push(f);
            }
        }
        for (hash, mut copies) in by_hash.into_iter().filter(|(_, v)| v.len() > 1) {
            copies.sort_by(|a, b| keep_order(places, a, b));
            let size = copies[0].size;
            let name = file_name(&copies[0].path);
            groups.push(DuplicateGroup {
                hash,
                name,
                size,
                copies: copies
                    .into_iter()
                    .map(|f| Copy {
                        path: f.path.to_string_lossy().into_owned(),
                        place: places[f.place].label.clone(),
                        kind: places[f.place].kind,
                        mtime: f.mtime,
                    })
                    .collect(),
            });
        }
    }
    groups.sort_by(|a, b| {
        let waste = |g: &DuplicateGroup| g.size * (g.copies.len() as u64 - 1);
        waste(b)
            .cmp(&waste(a))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Duplicates {
        groups,
        files,
        places: places.iter().map(|p| p.label.clone()).collect(),
    }
}

/// Copy to keep first: best kind of place, then the shallowest path (the
/// one a plug-in's browser shows at its top level), then the oldest, then A
/// to Z.
fn keep_order(places: &[Place], a: &Found, b: &Found) -> std::cmp::Ordering {
    let depth = |f: &Found| {
        f.path
            .strip_prefix(&places[f.place].path)
            .map_or(usize::MAX, |r| r.components().count())
    };
    places[a.place]
        .kind
        .cmp(&places[b.place].kind)
        .then_with(|| depth(a).cmp(&depth(b)))
        .then_with(|| {
            a.mtime
                .unwrap_or(i64::MAX)
                .cmp(&b.mtime.unwrap_or(i64::MAX))
        })
        .then_with(|| a.path.cmp(&b.path))
}

/// Keep one copy, remove the others.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Choice {
    pub keep: String,
    pub remove: Vec<String>,
}

/// What [`clean_up`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct CleanUp {
    /// Copies removed; History can put them back.
    pub removed: usize,
    /// Bytes freed.
    pub freed: u64,
    /// Copies left alone because they no longer match the kept file (or it's
    /// gone), or aren't presets.
    pub skipped: Vec<String>,
    /// The restore point, `None` when nothing was removed.
    pub restore_point: Option<i64>,
}

#[derive(Debug, thiserror::Error)]
pub enum CleanUpError {
    #[error(
        "Close {} first: some copies are in its folders, and Patch Pilot never changes them while it's open.",
        .0.display_name()
    )]
    DawRunning(Daw),
    #[error(transparent)]
    Apply(#[from] crate::restore::ApplyError),
}

/// Remove the chosen copies as one restore point. Each copy is checked again
/// first: it's only removed if it's a preset and still the same as the copy
/// kept, so a file edited since the search is never lost. `places` decides
/// which DAW must be closed.
pub fn clean_up(
    store: &mut Store,
    blobs: &Path,
    machine_id: &str,
    places: &[Place],
    choices: &[Choice],
    now: i64,
) -> Result<CleanUp, CleanUpError> {
    let mut out = CleanUp::default();
    let mut changes = Vec::new();
    let mut daws = Vec::new();
    let mut taken = HashSet::new();
    for c in choices {
        let keep = Path::new(&c.keep);
        let kept = is_preset(keep).then(|| hash_file(keep)).flatten();
        for r in &c.remove {
            let path = PathBuf::from(r);
            let same = kept.is_some()
                && path != keep
                && is_preset(&path)
                && hash_file(&path) == kept
                && !same_file(keep, &path);
            if !same || !taken.insert(path.clone()) {
                out.skipped.push(r.clone());
                continue;
            }
            if let Some(d) = places
                .iter()
                .filter(|p| path.starts_with(&p.path))
                .find_map(|p| p.daw)
                && !daws.contains(&d)
            {
                daws.push(d);
            }
            out.freed += std::fs::metadata(&path).map_or(0, |m| m.len());
            changes.push(FileChange {
                path,
                contents: None,
            });
        }
    }
    if changes.is_empty() {
        return Ok(out);
    }
    if let Some(&d) = daws.iter().find(|d| daw_running(**d)) {
        return Err(CleanUpError::DawRunning(d));
    }
    let n = changes.len();
    let req = ChangeSet {
        machine_id: machine_id.to_owned(),
        label: format!(
            "Cleaned up {n} duplicate preset{}",
            if n == 1 { "" } else { "s" }
        ),
        daw: daws.first().copied(),
        changes,
        now,
    };
    let point = apply_changes(store, blobs, &req)?;
    out.removed = point.changed.len();
    out.restore_point = point.id;
    Ok(out)
}

pub fn is_preset(path: &Path) -> bool {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .is_some_and(|e| PRESET_EXTENSIONS.contains(&e.as_str()))
}

/// Hidden folders, OS bins, and folders full of code or app bundles.
fn skip_dir(e: &walkdir::DirEntry) -> bool {
    if !e.file_type().is_dir() {
        return false;
    }
    let name = e.file_name().to_string_lossy();
    name.starts_with('.')
        || name.starts_with('$')
        || name.ends_with(".app")
        || name.ends_with(".bundle")
        || name.ends_with(".vst3")
        || name.ends_with(".component")
        || matches!(
            name.as_ref(),
            "node_modules" | "target" | "Patch Pilot" | "Backups" | "Backup"
        )
}

fn hash_file(path: &Path) -> Option<String> {
    let mut h = blake3::Hasher::new();
    h.update_reader(std::fs::File::open(path).ok()?).ok()?;
    Some(h.finalize().to_hex().to_string())
}

/// Two names for one file (a hard link, or a case-insensitive disk): removing
/// one would remove the only copy.
fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => true,
    }
}

fn file_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn mtime(meta: &std::fs::Metadata) -> Option<i64> {
    meta.modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_secs()).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, content: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn places(home: &Path) -> Vec<Place> {
        vec![
            Place::new(
                home.join("Documents/Xfer/Serum Presets"),
                PlaceKind::Standard,
                "Serum presets",
            ),
            Place {
                daw: Some(Daw::Reaper),
                ..Place::new(
                    home.join("Reaper/FXChains"),
                    PlaceKind::Daw,
                    "Reaper FX chains",
                )
            },
            Place::new(home.join("Documents"), PlaceKind::Personal, "Documents"),
            Place::new(home.join("Downloads"), PlaceKind::Loose, "Downloads"),
        ]
    }

    #[test]
    fn finds_copies_by_content_and_keeps_the_standard_one() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        write(&home.join("Downloads/pack/Big Bass.fxp"), "bass");
        write(
            &home.join("Documents/Xfer/Serum Presets/Bass/Big Bass.fxp"),
            "bass",
        );
        write(&home.join("Documents/old/renamed.fxp"), "bass");
        // Same size, other contents: not a duplicate.
        write(&home.join("Downloads/Lead.fxp"), "lead");
        // Same contents, not a preset: ignored.
        write(&home.join("Downloads/notes.txt"), "bass");
        // Hidden folders are skipped.
        write(&home.join("Documents/.Trash/Big Bass.fxp"), "bass");
        write(&home.join("Reaper/FXChains/Glue.RfxChain"), "<chain>");
        write(&home.join("Desktop/Glue.RfxChain"), "<chain>");
        write(&home.join("Documents/Glue copy.RfxChain"), "<chain>");

        let found = find_duplicates(&places(home));
        assert_eq!(found.files, 6);
        assert_eq!(found.groups.len(), 2);
        let bass = found
            .groups
            .iter()
            .find(|g| g.name == "Big Bass.fxp")
            .unwrap();
        let where_: Vec<&str> = bass.copies.iter().map(|c| c.place.as_str()).collect();
        // Documents/Xfer/... is counted for Serum, not again for Documents.
        assert_eq!(where_, ["Serum presets", "Documents", "Downloads"]);
        assert_eq!(bass.size, 4);
        let chain = found
            .groups
            .iter()
            .find(|g| g.name == "Glue.RfxChain")
            .unwrap();
        assert_eq!(chain.copies.len(), 2, "Desktop isn't a place here");
        assert_eq!(chain.copies[0].kind, PlaceKind::Daw);
        // Bigger waste first.
        assert_eq!(found.groups[0].name, "Big Bass.fxp");
    }

    #[test]
    fn clean_up_removes_only_matching_copies_and_can_be_undone() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let keep = home.join("Documents/Xfer/Serum Presets/Pad.fxp");
        let dup = home.join("Downloads/Pad.fxp");
        let edited = home.join("Documents/Pad.fxp");
        write(&keep, "pad");
        write(&dup, "pad");
        write(&edited, "pad");
        let found = find_duplicates(&places(&home));
        assert_eq!(found.groups[0].copies.len(), 3);
        // Edited after the search: must survive.
        write(&edited, "pad v2");

        let mut store = Store::open_in_memory().unwrap();
        let blobs = tmp.path().join("blobs");
        let path = |p: &Path| p.to_string_lossy().into_owned();
        let choices = [Choice {
            keep: path(&keep),
            remove: vec![path(&dup), path(&edited), path(&keep)],
        }];
        let done = clean_up(&mut store, &blobs, "m1", &places(&home), &choices, 100).unwrap();
        assert_eq!(done.removed, 1);
        assert_eq!(done.freed, 3);
        assert_eq!(done.skipped, [path(&edited), path(&keep)]);
        assert!(keep.exists() && edited.exists() && !dup.exists());

        let id = done.restore_point.unwrap();
        let point = store.restore_point(id).unwrap().unwrap();
        assert_eq!(point.label, "Cleaned up 1 duplicate preset");
        crate::restore::undo(&mut store, &blobs, id, 200).unwrap();
        assert_eq!(std::fs::read_to_string(&dup).unwrap(), "pad");
    }

    #[test]
    fn nothing_chosen_writes_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = Store::open_in_memory().unwrap();
        let done = clean_up(&mut store, tmp.path(), "m1", &[], &[], 1).unwrap();
        assert_eq!(done, CleanUp::default());
    }

    #[test]
    fn preset_extensions_ignore_case() {
        assert!(is_preset(Path::new("a/B.VSTPRESET")));
        assert!(is_preset(Path::new("Rack.adg")));
        assert!(!is_preset(Path::new("Song.rpp")));
        assert!(!is_preset(Path::new("Serum")));
    }
}
