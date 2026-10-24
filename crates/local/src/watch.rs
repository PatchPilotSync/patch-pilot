//! Watching this machine's plug-in and template folders, so the app can
//! rescan on its own when something is installed, updated or removed.
//!
//! Uses the OS's change notifications (FSEvents on macOS,
//! ReadDirectoryChangesW on Windows), never polling, so an idle machine costs
//! nothing. Changes are batched: an installer writes thousands of files, and
//! the callback runs once the folders have been quiet for a while.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::Duration;

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use patchpilot_core::Os;
use patchpilot_scanner::{SystemDirs, default_roots};

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
    folders.retain(|f| f.is_dir());
    outermost(folders)
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
    let (tx, rx) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<Event>| {
        // Reads (our own scans included) aren't changes.
        if event.is_ok_and(|e| !matches!(e.kind, EventKind::Access(_))) {
            let _ = tx.send(());
        }
    })?;
    let folders = folders
        .into_iter()
        .filter(|f| watcher.watch(f, RecursiveMode::Recursive).is_ok())
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
