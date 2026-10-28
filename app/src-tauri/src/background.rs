//! Patch Pilot as an always-on observer (decision #39). It opens at login
//! (hidden, in the tray), watches the plug-in and template folders, and
//! rescans quietly a little while after they change, so every rig's score is
//! current without the user pressing Rescan.

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use patchpilot_local::FolderWatch;
use patchpilot_local::app_prefs::Rescan;
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
/// After an update the last scan was made by older code (e.g. presets it now
/// skips), so rescan almost at once, with the status showing.
const AFTER_UPDATE_SCAN_DELAY: Duration = Duration::from_secs(3);
/// Emitted with `true` when the after-update rescan starts and `false` when
/// it ends, so the window and popover show "Scanning…".
pub const UPDATE_SCAN: &str = "update-scan";
/// How often to look again while a scan the user started is still running.
const BUSY_RETRY: Duration = Duration::from_secs(2);
/// Settings' "Once a day too" (decision #97): a sleeping thread, no polling.
const DAILY: Duration = Duration::from_secs(24 * 60 * 60);

pub struct Watcher {
    /// `true` asks for the visible after-update rescan.
    rescan: Sender<bool>,
    /// Set while the after-update rescan runs.
    update_scan: AtomicBool,
    /// Plug-in, template and preset folders, recursively.
    watch: Mutex<Option<FolderWatch>>,
    /// Where vendor apps and licensing tools land, one level deep.
    apps: Mutex<Option<FolderWatch>>,
    /// Reaper's and Live's settings files, by name (their folders are busy).
    settings: Mutex<Option<FolderWatch>>,
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
        update_scan: AtomicBool::new(false),
        watch: Mutex::new(None),
        apps: Mutex::new(None),
        settings: Mutex::new(None),
    });
    let handle = app.clone();
    let _ = thread::Builder::new()
        .name("patchpilot-rescan".to_owned())
        .spawn(move || rescans(&handle, &rx));
    // Settings (decision #97): the after-update rescan can be turned off, and
    // "Only when I press Rescan" skips the catch-up scan too.
    let prefs = crate::settings::app_prefs();
    let updated = prefs.rescan_after_update && scanned_by_older_version(app);
    if updated || prefs.rescan != Rescan::Manual {
        let tx = tx.clone();
        let _ = thread::Builder::new()
            .name("patchpilot-startup-scan".to_owned())
            .spawn(move || {
                thread::sleep(if updated {
                    AFTER_UPDATE_SCAN_DELAY
                } else {
                    STARTUP_SCAN_DELAY
                });
                let _ = tx.send(updated);
            });
    }
    let _ = thread::Builder::new()
        .name("patchpilot-daily-scan".to_owned())
        .spawn(move || {
            loop {
                thread::sleep(DAILY);
                if crate::settings::app_prefs().rescan == Rescan::Daily && tx.send(false).is_err() {
                    break;
                }
            }
        });
    rewatch(app);
}

/// Settings changed when to rescan: start or stop watching to match.
pub fn prefs_changed(app: &AppHandle) {
    if app.try_state::<Watcher>().is_some() {
        rewatch(app);
    }
}

/// True when this machine's latest scan wasn't made by this version of
/// Patch Pilot: the first launch after an update, or after updating from a
/// version that didn't record it.
fn scanned_by_older_version(app: &AppHandle) -> bool {
    let Ok(db) = rig::db_path() else {
        return false;
    };
    let dir = patchpilot_local::app_dir(&db);
    // Never scanned: the welcome screen's first scan comes first.
    if patchpilot_local::up_to_speed::last_check(dir).is_none() {
        return false;
    }
    let current = app.package_info().version.to_string();
    patchpilot_local::scanned_with(dir).as_deref() != Some(current.as_str())
}

/// True while the after-update rescan runs, for a window opened meanwhile.
#[tauri::command]
pub fn update_scan_running(watcher: tauri::State<'_, Watcher>) -> bool {
    watcher.update_scan.load(Ordering::SeqCst)
}

/// (Re)start the watches when the set of folders changed, e.g. an installer
/// created the first CLAP folder.
fn rewatch(app: &AppHandle) {
    let state = app.state::<Watcher>();
    if crate::settings::app_prefs().rescan == Rescan::Manual {
        for slot in [&state.watch, &state.apps, &state.settings] {
            if let Ok(mut w) = slot.lock() {
                *w = None;
            }
        }
        set_tooltip(app, &[]);
        return;
    }
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
    restart(
        &state.settings,
        patchpilot_local::settings_files(),
        &state.rescan,
        patchpilot_local::watch_files,
    );
    if let Some(watching) = watching {
        set_tooltip(app, &watching);
    }
}

/// Returns the folders now watched, or None when nothing changed.
fn restart<W, E>(
    slot: &Mutex<Option<FolderWatch>>,
    folders: Vec<PathBuf>,
    rescan: &Sender<bool>,
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
            let _ = rescan.send(false);
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

/// Rescan soon, e.g. after files were placed here: settings files written
/// while a folder wasn't watched would otherwise wait for the next change.
pub fn rescan_soon(app: &AppHandle) {
    if let Some(state) = app.try_state::<Watcher>() {
        let _ = state.rescan.send(false);
    }
}

/// One scan at a time, however many changes arrive meanwhile.
fn rescans(app: &AppHandle, rx: &Receiver<bool>) {
    let watcher = app.state::<Watcher>();
    while let Ok(mut visible) = rx.recv() {
        let scanning = app.state::<Scanning>();
        while !scanning.start() {
            thread::sleep(BUSY_RETRY);
        }
        // Changes that arrived while waiting are covered by this scan.
        while let Ok(v) = rx.try_recv() {
            visible |= v;
        }
        if visible {
            watcher.update_scan.store(true, Ordering::SeqCst);
            let _ = app.emit(UPDATE_SCAN, true);
        }
        let result = rig::rescan_existing(app, visible);
        scanning.finish();
        if visible {
            watcher.update_scan.store(false, Ordering::SeqCst);
            let _ = app.emit(UPDATE_SCAN, false);
        }
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
