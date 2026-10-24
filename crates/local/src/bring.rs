//! Bring over (decision #45): copy the Golden Rig's user-owned files, such as
//! Reaper templates, FX chains, scripts and themes, onto this machine. Never plug-ins (CLAUDE.md
//! rule 2), never silently: the user picks the files, anything replaced is
//! backed up to app-data first, and each file is staged and renamed into place.
//!
//! No network here: the app fetches each file's bytes from the relay, sealed
//! under the rig key and checked against the hash Golden recorded.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use patchpilot_core::{ContentHash, Item, Manifest, Os, PortablePath, TargetProfile};
use patchpilot_parity::ableton::{
    ABLETON_PREFS, ABLETON_USER_LIBRARY, prefs_label, user_library_label,
};
use patchpilot_parity::apply_profile;
use patchpilot_parity::reaper::{REAPER_RESOURCES, reaper_resource_dir, resource_kind};
use patchpilot_store::Store;
use serde::Serialize;

use crate::restore::{ChangeSet, FileChange, apply_changes};

/// The relay keeps objects up to 2 MB; leave room for the envelope.
pub const MAX_FILE_BYTES: u64 = 1_900_000;

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
    },
    // Ableton (decision #41): Live indexes the User Library and reads
    // Options.txt at launch, so both wait until Live is closed.
    SharedRoot {
        root: ABLETON_USER_LIBRARY,
        app: "Ableton",
        kind: user_library_label,
        default_dir: || crate::standard_ableton_dirs()?.user_library,
        guard: || crate::apps::write_guard(ABLETON_USER_LIBRARY),
    },
    SharedRoot {
        root: ABLETON_PREFS,
        app: "Ableton",
        kind: prefs_label,
        default_dir: || crate::standard_ableton_dirs()?.prefs,
        guard: || crate::apps::write_guard(ABLETON_PREFS),
    },
];

fn shared_root(root: &str) -> Option<&'static SharedRoot> {
    SHARED_ROOTS.iter().find(|r| r.root == root)
}

/// The shared root and label of a path, if Bring over handles it.
fn classify(path: &PortablePath) -> Option<(&'static SharedRoot, &str, &'static str)> {
    let PortablePath::Logical { root, rel } = path else {
        return None;
    };
    let shared = shared_root(root)?;
    Some((shared, rel, (shared.kind)(rel)?))
}

/// One file Golden has that this machine lacks or has a different copy of.
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
}

/// A user-owned file this machine shares with its linked machines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedFile {
    pub hash: ContentHash,
    pub path: PathBuf,
}

/// The files on this machine others may bring over: every item in its latest
/// scan under a [`SHARED_ROOTS`] root, with a hash, that fits the relay.
pub fn shared_files(machine: &Manifest) -> Vec<SharedFile> {
    machine
        .items
        .iter()
        .filter(|i| i.size.is_some_and(|s| s <= MAX_FILE_BYTES))
        .filter_map(|i| {
            classify(&i.path)?;
            let path = machine.machine.path_map.resolve(&i.path)?;
            Some(SharedFile {
                hash: i.content_hash.clone()?,
                path: PathBuf::from(path),
            })
        })
        .collect()
}

/// What Golden (narrowed by this machine's profile) has that `machine` lacks
/// or holds a different copy of, sorted by app, then path. `machine` is
/// `None` before this machine's first scan.
pub fn files_to_bring(
    golden: &Manifest,
    profile: &TargetProfile,
    machine: Option<&Manifest>,
) -> Vec<FileToBring> {
    let target = apply_profile(golden, profile);
    let os = machine.map(|m| m.machine.os);
    // Paths compare case-insensitively (NTFS and APFS both are). When both
    // Golden slots have a file, prefer this OS's.
    let key = |root: &str, rel: &str| (root.to_owned(), rel.to_ascii_lowercase());
    let mut wanted: BTreeMap<(String, String), (Os, &Item)> = BTreeMap::new();
    for item in &target.items {
        let Some((shared, rel, _)) = classify(&item.path) else {
            continue;
        };
        if item.content_hash.is_none() {
            continue;
        }
        let k = key(shared.root, rel);
        let better = match wanted.get(&k) {
            None => true,
            Some((have, _)) => Some(*have) != os && Some(item.platform) == os,
        };
        if better {
            wanted.insert(k, (item.platform, item));
        }
    }
    let here: BTreeMap<(String, String), &ContentHash> = machine
        .into_iter()
        .flat_map(|m| &m.items)
        .filter_map(|i| {
            let (shared, rel, _) = classify(&i.path)?;
            Some((key(shared.root, rel), i.content_hash.as_ref()?))
        })
        .collect();

    let mut out: Vec<FileToBring> = wanted
        .into_iter()
        .filter_map(|(k, (_, item))| {
            let hash = item.content_hash.clone()?;
            let replaces = match here.get(&k) {
                Some(h) if **h == hash => return None,
                Some(_) => true,
                None => false,
            };
            let (shared, rel, kind) = classify(&item.path)?;
            Some(FileToBring {
                app: shared.app.to_owned(),
                kind: kind.to_owned(),
                root: shared.root.to_owned(),
                rel: rel.to_owned(),
                too_large: item.size.is_some_and(|s| s > MAX_FILE_BYTES),
                size: item.size,
                hash,
                replaces,
            })
        })
        .collect();
    out.sort_by(|a, b| (&a.app, &a.rel).cmp(&(&b.app, &b.rel)));
    out
}

/// Where `root` is on this machine: as its last scan mapped it, else the
/// root's default place.
pub fn root_dir(machine: Option<&Manifest>, root: &str) -> Option<PathBuf> {
    if let Some(dir) = machine.and_then(|m| m.machine.path_map.roots.get(root)) {
        return Some(PathBuf::from(dir));
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

/// What [`write_files`] did. Mirrored in `app/src/api.ts`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct WriteReport {
    pub written: usize,
    /// Files that were replaced; their old bytes are in the restore point.
    pub backed_up: usize,
    /// The restore point History can undo, if anything was written.
    pub restore_point: Option<i64>,
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
    let shared = shared_root(root).ok_or_else(|| format!("{root}: not a shared folder"))?;
    (shared.guard)()?;
    let mut changes = Vec::new();
    let mut backed_up = 0;
    for (rel, bytes) in files {
        if (shared.kind)(rel).is_none() {
            return Err(format!("{rel}: not a file {} can receive", shared.app));
        }
        let target = rel.split('/').fold(dir.to_owned(), |p, part| p.join(part));
        match std::fs::read(&target) {
            Ok(old) if old == *bytes => continue,
            Ok(_) => backed_up += 1,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("{}: {e}", target.display())),
        }
        changes.push(FileChange {
            path: target,
            contents: Some(bytes.clone()),
        });
    }
    let req = ChangeSet {
        machine_id: machine_id.to_owned(),
        label: label.to_owned(),
        // Templates, scripts and themes: Reaper doesn't rewrite them on
        // exit, so it may stay open (the root's guard covers the rest).
        daw: None,
        changes,
        now,
    };
    let point = apply_changes(store, blobs, &req).map_err(|e| e.to_string())?;
    Ok(WriteReport {
        written: point.changed.len(),
        backed_up,
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
        // Nothing to bring onto the machine Golden came from.
        assert!(files_to_bring(&golden, &TargetProfile::default(), Some(&golden)).is_empty());
        // Before a first scan: everything.
        assert_eq!(
            files_to_bring(&golden, &TargetProfile::default(), None).len(),
            4
        );
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
            (ABLETON_PREFS, "Library.cfg"),
            (ABLETON_USER_LIBRARY, "Samples/Imported/kick.wav"),
        ] {
            let dir = tmp.path().join(root);
            let result = write_files(
                root,
                &dir,
                &tmp.path().join("b"),
                &[(rel.into(), b"x".to_vec())],
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
                },
                SharedFile {
                    hash: hash("nudge"),
                    path: PathBuf::from(format!("{dir}/Scripts/Tools/Nudge.lua")),
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
}
