//! Patch Pilot as an always-on observer (decision #39). It opens at login
//! (hidden, in the tray), watches the plug-in and template folders, and
//! rescans quietly a little while after they change, so every rig's score is
//! current without the user pressing Rescan.

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use patchpilot_local::FolderWatch;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_autostart::ManagerExt;

use crate::rig::{self, Scanning};

/// Passed by the login item, so the app starts in the tray without a window.
pub const BACKGROUND_ARG: &str = "--background";

/// How long the folders must stay quiet after a change before rescanning;
/// installers write in bursts.
const QUIET: Duration = Duration::from_secs(20);
/// The catch-up scan for changes made while Patch Pilot wasn't running waits
/// this long after launch, to stay out of the way at login.
const STARTUP_SCAN_DELAY: Duration = Duration::from_secs(60);
/// How often to look again while a scan the user started is still running.
const BUSY_RETRY: Duration = Duration::from_secs(2);

pub struct Watcher {
    rescan: Sender<()>,
    /// Plug-in, template and preset folders, recursively.
    watch: Mutex<Option<FolderWatch>>,
    /// Where vendor apps and licensing tools land, one level deep.
    apps: Mutex<Option<FolderWatch>>,
}

/// True when this launch came from the login item.
pub fn launched_at_login() -> bool {
    std::env::args().any(|a| a == BACKGROUND_ARG)
}

/// Start watching and schedule the catch-up scan.
pub fn start(app: &AppHandle) {
    let (tx, rx) = mpsc::channel();
    app.manage(Watcher {
        rescan: tx.clone(),
        watch: Mutex::new(None),
        apps: Mutex::new(None),
    });
    let handle = app.clone();
    let _ = thread::Builder::new()
        .name("patchpilot-rescan".to_owned())
        .spawn(move || rescans(&handle, &rx));
    let _ = thread::Builder::new()
        .name("patchpilot-startup-scan".to_owned())
        .spawn(move || {
            thread::sleep(STARTUP_SCAN_DELAY);
            let _ = tx.send(());
        });
    rewatch(app);
}

/// (Re)start the watches when the set of folders changed, e.g. an installer
/// created the first CLAP folder.
fn rewatch(app: &AppHandle) {
    let state = app.state::<Watcher>();
    let watching = restart(
        &state.watch,
        patchpilot_local::watched_folders(),
        &state.rescan,
        patchpilot_local::watch_folders,
    );
    restart(
        &state.apps,
        patchpilot_local::app_folders(),
        &state.rescan,
        patchpilot_local::watch_folders_flat,
    );
    if let Some(watching) = watching {
        set_tooltip(app, &watching);
    }
}

/// Returns the folders now watched, or None when nothing changed.
fn restart<W, E>(
    slot: &Mutex<Option<FolderWatch>>,
    folders: Vec<PathBuf>,
    rescan: &Sender<()>,
    watch: W,
) -> Option<Vec<PathBuf>>
where
    W: FnOnce(Vec<PathBuf>, Duration, Box<dyn FnMut() + Send>) -> Result<FolderWatch, E>,
{
    let mut current = slot.lock().ok()?;
    if current.as_ref().is_some_and(|w| w.folders == folders) {
        return None;
    }
    // Stop the old watch before starting the new one.
    *current = None;
    let rescan = rescan.clone();
    *current = watch(
        folders,
        QUIET,
        Box::new(move || {
            let _ = rescan.send(());
        }),
    )
    .ok();
    Some(
        current
            .as_ref()
            .map(|w| w.folders.clone())
            .unwrap_or_default(),
    )
}

fn set_tooltip(app: &AppHandle, watching: &[PathBuf]) {
    let text = if watching.is_empty() {
        "Patch Pilot".to_owned()
    } else {
        "Patch Pilot: watching your plug-in folders".to_owned()
    };
    if let Some(tray) = app.tray_by_id(crate::tray::ID) {
        let _ = tray.set_tooltip(Some(text));
    }
}

/// One scan at a time, however many changes arrive meanwhile.
fn rescans(app: &AppHandle, rx: &Receiver<()>) {
    while rx.recv().is_ok() {
        let scanning = app.state::<Scanning>();
        while !scanning.start() {
            thread::sleep(BUSY_RETRY);
        }
        // Changes that arrived while waiting are covered by this scan.
        while rx.try_recv().is_ok() {}
        let result = rig::rescan_quietly(app);
        scanning.finish();
        if let Ok(Some(report)) = result
            && !report.unchanged
        {
            // The open window and popover refresh on this, as after a tray Rescan.
            let _ = app.emit("data-changed", ());
        }
        rewatch(app);
    }
}

/// Turn "Open at Login" on the first time Patch Pilot runs, and never again:
/// after that it's the user's switch (tray menu, Windows Task Manager, macOS
/// Login Items).
pub fn apply_login_default(app: &AppHandle) {
    // A development build would register target/debug as the login item.
    if cfg!(debug_assertions) || !installed() {
        return;
    }
    let Ok(db) = rig::db_path() else {
        return;
    };
    let dir = patchpilot_local::app_dir(&db);
    if patchpilot_local::login_default_applied(dir) {
        return;
    }
    if app.autolaunch().enable().is_ok() {
        let _ = patchpilot_local::mark_login_default_applied(dir);
    }
}

/// False when macOS runs the app from the disk image or a quarantined
/// download (App Translocation): that path disappears, so a login item for it
/// would break. It's set on a later launch from Applications instead.
fn installed() -> bool {
    if !cfg!(target_os = "macos") {
        return true;
    }
    std::env::current_exe().is_ok_and(|exe| {
        let exe = exe.to_string_lossy();
        !exe.starts_with("/Volumes/") && !exe.contains("/AppTranslocation/")
    })
}
