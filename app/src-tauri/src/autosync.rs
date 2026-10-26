//! Keep files in sync (decision #70): new and changed scripts, templates and
//! presets from linked machines are placed here on their own, through the
//! backup folder. This machine backs up after every change (backup.rs); here
//! it watches that folder (not polling), and looks again at launch and when a
//! linked machine's scan arrives, then places what's new through restore
//! points and says so in one quiet notification. The logic lives in
//! `patchpilot_local::autosync`.

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use patchpilot_local::FolderWatch;
use patchpilot_local::autosync::{self, Conflict, SyncReport};
use patchpilot_local::backup::{self, backup_dir};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Listener, Manager};
use tauri_plugin_notification::NotificationExt;

use crate::backup::BACKUP_CHANGED;
use crate::rig::{db_path, open};
use crate::sync::{app_dir, this_machine};

/// Cloud apps write a backup in pieces; wait for them to settle.
const QUIET: Duration = Duration::from_secs(10);
/// How long to wait before trying again while a DAW is open.
const RETRY: Duration = Duration::from_secs(120);
/// The first look after launch waits for the catch-up scan and backup.
const STARTUP_DELAY: Duration = Duration::from_secs(90);
/// Emitted after files were placed or a conflict found, for the UI.
pub const SYNC_CHANGED: &str = "sync-changed";

#[derive(Default)]
struct SyncState {
    /// The backup folder watched, and its watch.
    watch: Mutex<Option<(PathBuf, FolderWatch)>>,
    /// One look at a time.
    running: Mutex<()>,
    last: Mutex<Option<SyncReport>>,
    error: Mutex<Option<String>>,
    retry_pending: AtomicBool,
}

/// Mirrored in `src/api.ts`.
#[derive(Debug, Serialize)]
pub struct SyncView {
    pub on: bool,
    /// A backup folder is picked and there.
    pub folder_ready: bool,
    /// Other machines linked to this one.
    pub linked: usize,
    pub conflicts: Vec<Conflict>,
    pub last: Option<SyncReport>,
    pub error: Option<String>,
}

pub fn setup(app: &AppHandle) {
    app.manage(SyncState::default());
    rewatch(app);
    // The backup folder may have been picked or changed.
    let handle = app.clone();
    app.listen_any(BACKUP_CHANGED, move |_| rewatch(&handle));
    // A linked machine's scan often arrives before its files do; its backup
    // may be there already.
    let handle = app.clone();
    app.listen_any("data-changed", move |_| {
        let handle = handle.clone();
        std::thread::spawn(move || look(&handle));
    });
    let handle = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(STARTUP_DELAY);
        look(&handle);
    });
}

pub fn enabled() -> bool {
    app_dir().is_ok_and(|d| backup::load_settings(&d).keep_in_sync)
}

/// The `Patch Pilot Backup` folder, when keeping in sync is on and it's there.
fn sync_dir() -> Option<PathBuf> {
    let settings = backup::load_settings(&app_dir().ok()?);
    if !settings.keep_in_sync {
        return None;
    }
    backup_dir(&settings).filter(|d| d.is_dir())
}

fn rewatch(app: &AppHandle) {
    let dir = sync_dir();
    let state = app.state::<SyncState>();
    let Ok(mut slot) = state.watch.lock() else {
        return;
    };
    if slot.as_ref().map(|(d, _)| d) == dir.as_ref() {
        return;
    }
    *slot = None;
    let Some(dir) = dir else {
        return;
    };
    let handle = app.clone();
    if let Ok(w) = patchpilot_local::watch_folders(vec![dir.clone()], QUIET, move || look(&handle))
    {
        *slot = Some((dir, w));
    }
}

/// Place what's new from linked machines, then tell the user. Quiet on
/// errors: the next change tries again, and the UI shows the last one.
pub fn look(app: &AppHandle) {
    let state = app.state::<SyncState>();
    let Ok(_one) = state.running.lock() else {
        return;
    };
    let result = sync_now(app);
    if let Ok(mut e) = state.error.lock() {
        *e = result.as_ref().err().cloned();
    }
    let Ok(Some(report)) = result else {
        return;
    };
    if let Ok(mut last) = state.last.lock() {
        *last = Some(report.clone());
    }
    if !report.waiting.is_empty() && !state.retry_pending.swap(true, Ordering::SeqCst) {
        let handle = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(RETRY);
            handle
                .state::<SyncState>()
                .retry_pending
                .store(false, Ordering::SeqCst);
            look(&handle);
        });
    }
    if report.placed.is_empty() && report.new_conflicts == 0 {
        return;
    }
    notify(app, &report);
    let _ = app.emit(SYNC_CHANGED, ());
    // The template and preset folders are watched: a rescan (and the
    // backup that sends the files on) follows the writes on its own.
    let _ = app.emit("data-changed", ());
}

fn sync_now(app: &AppHandle) -> Result<Option<SyncReport>, String> {
    let Some(dir) = sync_dir() else {
        return Ok(None);
    };
    // With GitHub, bring the copy up to date first (decision #72).
    crate::github::sync_now(app)?;
    let db = db_path()?;
    if !db.exists() {
        return Ok(None);
    }
    let this = this_machine()?;
    let app_dir = app_dir()?;
    let mut store = open(&db)?;
    let mut state = autosync::load_state(&app_dir);
    let report = autosync::sync_in(
        &mut store,
        &app_dir.join("blobs"),
        &this,
        &dir,
        &mut state,
        patchpilot_local::now(),
    )?;
    autosync::save_state(&app_dir, &state)?;
    Ok(Some(report))
}

fn notify(app: &AppHandle, report: &SyncReport) {
    let state = autosync::load_state(&app_dir().unwrap_or_default());
    let mut lines = Vec::new();
    if !report.placed.is_empty() {
        lines.push(placed_summary(report));
    }
    if report.new_conflicts > 0 {
        lines.push(match state.conflicts.as_slice() {
            [c] => format!(
                "{} changed here and on {}: open Patch Pilot to choose.",
                file_name(&c.rel),
                c.machine_name
            ),
            all => format!(
                "{} files changed here and on another machine: open Patch Pilot to choose.",
                all.len()
            ),
        });
    }
    // Best effort: a missing notification permission shouldn't break anything.
    let _ = app
        .notification()
        .builder()
        .title("Files kept in sync")
        .body(lines.join(" "))
        .show();
}

/// "3 Reaper files and 2 presets from MacBook. Undo in History."
fn placed_summary(report: &SyncReport) -> String {
    let mut from: Vec<&str> = report
        .placed
        .iter()
        .map(|p| p.machine_name.as_str())
        .collect();
    from.dedup();
    let mut parts: Vec<String> = Vec::new();
    for p in &report.placed {
        let what = if p.app == "Presets" {
            "preset".to_owned()
        } else {
            format!("{} file", p.app)
        };
        parts.push(format!(
            "{} {what}{}",
            p.files,
            if p.files == 1 { "" } else { "s" }
        ));
    }
    format!(
        "{} from {}. Undo in History.",
        parts.join(", "),
        from.join(", ")
    )
}

fn file_name(rel: &str) -> &str {
    rel.rsplit('/').next().unwrap_or(rel)
}

#[tauri::command]
pub async fn sync_view(app: AppHandle) -> Result<SyncView, String> {
    let dir = app_dir()?;
    let settings = backup::load_settings(&dir);
    let state = app.state::<SyncState>();
    let this = this_machine().ok();
    let linked = match db_path() {
        Ok(db) if db.exists() => open(&db)?
            .machines()
            .map_err(|e| e.to_string())?
            .into_iter()
            .filter(|m| Some(&m.id) != this.as_ref())
            .count(),
        _ => 0,
    };
    Ok(SyncView {
        on: settings.keep_in_sync,
        folder_ready: backup_dir(&settings).is_some(),
        linked,
        conflicts: autosync::load_state(&dir).conflicts,
        last: state.last.lock().ok().and_then(|l| l.clone()),
        error: state.error.lock().ok().and_then(|e| e.clone()),
    })
}

/// Turn keeping files in sync on or off. On also backs up after every change,
/// backs up now and looks for new files at once.
#[tauri::command]
pub async fn set_keep_in_sync(app: AppHandle, on: bool) -> Result<SyncView, String> {
    set_enabled(&app, on)?;
    sync_view(app).await
}

pub fn set_enabled(app: &AppHandle, on: bool) -> Result<(), String> {
    let dir = app_dir()?;
    let mut settings = backup::load_settings(&dir);
    if on && backup_dir(&settings).is_none() {
        return Err("Pick your cloud folder first.".into());
    }
    settings.keep_in_sync = on;
    backup::save_settings(&dir, &settings)?;
    let _ = app.emit(BACKUP_CHANGED, ());
    crate::tray::refresh_sync_check(app);
    if on {
        let handle = app.clone();
        std::thread::spawn(move || {
            crate::backup::auto_back_up(&handle);
            look(&handle);
        });
    }
    Ok(())
}

/// Settle a file changed here and on `machine_id`: use that machine's copy
/// (`theirs`) or keep this one.
#[tauri::command]
pub async fn settle_conflict(
    app: AppHandle,
    root: String,
    rel: String,
    machine_id: String,
    theirs: bool,
) -> Result<SyncView, String> {
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = handle.state::<SyncState>();
        let _one = state.running.lock().map_err(|e| e.to_string())?;
        let dir = backup_dir(&backup::load_settings(&app_dir()?))
            .ok_or("The cloud folder isn't there now (signed out, or not connected).")?;
        let app_dir = app_dir()?;
        let mut store = open(&db_path()?)?;
        let mut sync = autosync::load_state(&app_dir);
        autosync::settle(
            &mut store,
            &app_dir.join("blobs"),
            &this_machine()?,
            &dir,
            &mut sync,
            &root,
            &rel,
            &machine_id,
            theirs,
            patchpilot_local::now(),
        )?;
        autosync::save_state(&app_dir, &sync)
    })
    .await
    .map_err(|e| e.to_string())??;
    let _ = app.emit("data-changed", ());
    sync_view(app).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use patchpilot_local::autosync::Placed;

    #[test]
    fn summarises_what_was_placed() {
        let placed = |app: &str, files| Placed {
            machine_name: "MacBook".into(),
            app: app.into(),
            files,
            new: files,
            restore_point: None,
        };
        let r = SyncReport {
            placed: vec![placed("Reaper", 3), placed("Presets", 1)],
            ..SyncReport::default()
        };
        assert_eq!(
            placed_summary(&r),
            "3 Reaper files, 1 preset from MacBook. Undo in History."
        );
    }
}
