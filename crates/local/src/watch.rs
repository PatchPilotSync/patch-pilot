//! Watching this machine's plug-in and template folders, so the app can
//! rescan on its own when something is installed, updated or removed, and
//! the Downloads folder for new installers (Installer Vault).
//!
//! Uses the OS's change notifications (FSEvents on macOS,
//! ReadDirectoryChangesW on Windows), never polling, so an idle machine costs
//! nothing. Changes are batched: an installer writes thousands of files, and
//! the callback runs once the folders have been quiet for a while.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::Duration;

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use patchpilot_core::Os;
use patchpilot_scanner::{ADAPTERS, SystemDirs, default_roots};

/// Reaper resource subfolders holding the templates a scan reads (#37).
const REAPER_TEMPLATE_DIRS: [&str; 5] = [
    "TrackTemplates",
    "ProjectTemplates",
    "FXChains",
    "Scripts",
    "ColorThemes",
];

/// User Library subfolders holding what a scan shares. Not `Samples/`
/// (Live writes freezes there), nor the preferences folder (Live writes its
/// log there all the time); `Options.txt` changes are picked up by the next
/// scan.
const ABLETON_WATCHED_DIRS: [&str; 6] = [
    "Templates",
    "Presets",
    "Defaults",
    "Clips",
    "Grooves",
    "Ableton Folder Info",
];

/// The folders a standard scan reads that exist right now: every default
/// plug-in folder, Reaper's template folders and the Ableton User Library's
/// preset and template folders. Folders inside another one in
/// the list are left out, since watching is recursive.
pub fn watched_folders() -> Vec<PathBuf> {
    let (Some(os), Some(dirs)) = (Os::current(), SystemDirs::from_env()) else {
        return Vec::new();
    };
    let mut folders: Vec<PathBuf> = default_roots(&dirs).into_iter().map(|r| r.path).collect();
    if let Some(reaper) = crate::default_reaper_dir(os, &dirs) {
        folders.extend(REAPER_TEMPLATE_DIRS.map(|d| reaper.join(d)));
    }
    if let Some(lib) = crate::default_ableton_dirs(os, &dirs).user_library {
        folders.extend(ABLETON_WATCHED_DIRS.map(|d| lib.join(d)));
    }
    folders.extend(crate::preset_folders(os).into_iter().map(|f| f.path));
    if os == Os::Windows {
        // Nearly every installer adds a Start menu shortcut, often in a
        // vendor subfolder; vendor apps themselves are found in the registry.
        for var in ["ProgramData", "APPDATA"] {
            if let Some(base) = std::env::var_os(var).filter(|v| !v.is_empty()) {
                folders.push(PathBuf::from(base).join(START_MENU_PROGRAMS));
            }
        }
    }
    folders.retain(|f| f.is_dir());
    outermost(folders)
}

const START_MENU_PROGRAMS: &str = r"Microsoft\Windows\Start Menu\Programs";

/// Where vendor apps and licensing tools are installed, to watch one level
/// deep: a new Native Access or iLok License Manager shows up there, while
/// writes inside other apps don't wake us. On Windows that's Program Files;
/// on macOS the folders holding each known app (`/Applications`,
/// `~/Applications`, `/Library/PrivilegedHelperTools` for PACE, ...).
pub fn app_folders() -> Vec<PathBuf> {
    let (Some(os), Some(dirs)) = (Os::current(), SystemDirs::from_env()) else {
        return Vec::new();
    };
    let mut folders = match os {
        Os::Macos => macos_app_folders(&dirs.home),
        Os::Windows => ["ProgramFiles", "ProgramFiles(x86)"]
            .into_iter()
            .filter_map(std::env::var_os)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .collect(),
    };
    folders.retain(|f| f.is_dir());
    folders
}

fn macos_app_folders(home: &str) -> Vec<PathBuf> {
    let mut roots = vec![PathBuf::from("/")];
    if !home.is_empty() {
        roots.push(PathBuf::from(home));
    }
    let mut folders: Vec<PathBuf> = ADAPTERS
        .iter()
        .flat_map(|a| a.macos_paths)
        .filter_map(|rel| Path::new(rel).parent())
        .flat_map(|parent| roots.iter().map(move |r| r.join(parent)))
        .collect();
    folders.sort();
    folders.dedup();
    folders
}

fn outermost(mut folders: Vec<PathBuf>) -> Vec<PathBuf> {
    folders.sort();
    folders.dedup();
    let all = folders.clone();
    folders.retain(|f| !all.iter().any(|o| o != f && f.starts_with(o)));
    folders
}

/// A running watch. Dropping it stops watching and ends its thread.
pub struct FolderWatch {
    _watcher: RecommendedWatcher,
    /// The folders actually being watched (ones that couldn't be are left out).
    pub folders: Vec<PathBuf>,
}

/// Watch `folders` recursively and call `on_change` (on a background thread)
/// after a change, once nothing more has changed for `quiet`.
pub fn watch_folders(
    folders: Vec<PathBuf>,
    quiet: Duration,
    on_change: impl FnMut() + Send + 'static,
) -> notify::Result<FolderWatch> {
    watch(folders, RecursiveMode::Recursive, quiet, on_change)
}

/// Like [`watch_folders`], but only the files directly in `folder` (the
/// Downloads folder, where unpacked archives shouldn't wake us).
pub fn watch_folder_flat(
    folder: PathBuf,
    quiet: Duration,
    on_change: impl FnMut() + Send + 'static,
) -> notify::Result<FolderWatch> {
    watch_folders_flat(vec![folder], quiet, on_change)
}

/// [`watch_folder_flat`] for several folders, e.g. [`app_folders`].
pub fn watch_folders_flat(
    folders: Vec<PathBuf>,
    quiet: Duration,
    on_change: impl FnMut() + Send + 'static,
) -> notify::Result<FolderWatch> {
    watch(folders, RecursiveMode::NonRecursive, quiet, on_change)
}

fn watch(
    folders: Vec<PathBuf>,
    mode: RecursiveMode,
    quiet: Duration,
    on_change: impl FnMut() + Send + 'static,
) -> notify::Result<FolderWatch> {
    let (tx, rx) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<Event>| {
        // Reads (our own scans included) aren't changes.
        if event.is_ok_and(|e| !matches!(e.kind, EventKind::Access(_))) {
            let _ = tx.send(());
        }
    })?;
    let folders = folders
        .into_iter()
        .filter(|f| watcher.watch(f, mode).is_ok())
        .collect();
    thread::Builder::new()
        .name("patchpilot-watch".to_owned())
        .spawn(move || settle(&rx, quiet, on_change))
        .map_err(notify::Error::io)?;
    Ok(FolderWatch {
        _watcher: watcher,
        folders,
    })
}

/// Wait for a change, then for `quiet` without one, then report it. Returns
/// when the watcher (the sending side) is dropped.
fn settle(rx: &Receiver<()>, quiet: Duration, mut on_change: impl FnMut()) {
    while rx.recv().is_ok() {
        loop {
            match rx.recv_timeout(quiet) {
                Ok(()) => continue,
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
        on_change();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn mac_app_folders_hold_every_known_app() {
        let got = macos_app_folders("/Users/a");
        for want in [
            "/Applications",
            "/Applications/iZotope",
            "/Library/PrivilegedHelperTools",
            "/Users/a/Applications",
        ] {
            assert!(got.contains(&PathBuf::from(want)), "{want} in {got:?}");
        }
        assert!(!got.contains(&PathBuf::from("/")));
    }

    #[test]
    fn nested_folders_are_covered_by_their_parent() {
        let p = |s: &str| PathBuf::from(s);
        let got = outermost(vec![
            p("/Library/Audio/Plug-Ins/VST3"),
            p("/Library/Audio/Plug-Ins"),
            p("/Library/Audio/Plug-Ins/VST3/Sub"),
            p("/Users/a/Library/Audio/Plug-Ins/VST3"),
            p("/Library/Audio/Plug-Ins"),
            p("/Library/Audio/Plug-InsExtra"),
        ]);
        assert_eq!(
            got,
            [
                p("/Library/Audio/Plug-Ins"),
                p("/Library/Audio/Plug-InsExtra"),
                p("/Users/a/Library/Audio/Plug-Ins/VST3"),
            ]
        );
    }

    #[test]
    fn a_burst_of_changes_is_reported_once_after_it_settles() {
        let dir = tempfile::tempdir().unwrap();
        // macOS hands out /var paths that FSEvents reports as /private/var.
        let root = dir.path().canonicalize().unwrap();
        let (tx, rx) = mpsc::channel();
        let watch = watch_folders(
            vec![root.clone(), root.join("missing")],
            Duration::from_millis(500),
            move || tx.send(()).unwrap(),
        )
        .unwrap();
        assert_eq!(watch.folders, std::slice::from_ref(&root));
        // Give the OS watcher a moment to start (FSEvents is asynchronous).
        thread::sleep(Duration::from_millis(300));

        let bundle = root.join("Synth.vst3").join("Contents");
        std::fs::create_dir_all(&bundle).unwrap();
        for i in 0..5 {
            write(&bundle.join(format!("file{i}")));
        }
        rx.recv_timeout(Duration::from_secs(20))
            .expect("a change is reported");
        assert!(
            rx.recv_timeout(Duration::from_millis(1500)).is_err(),
            "one report for the whole burst"
        );

        drop(watch);
        write(&root.join("after"));
        assert!(rx.recv_timeout(Duration::from_secs(2)).is_err());
    }

    fn write(path: &Path) {
        std::fs::write(path, b"x").unwrap();
    }
}
