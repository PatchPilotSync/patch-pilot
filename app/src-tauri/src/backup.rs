//! Back up everything (decision #67): this machine's templates, scripts,
//! themes, settings, presets and Installer Vault index go into `Patch Pilot
//! Backup` in a cloud folder it picks, by hand or after every changed scan,
//! and any machine can restore one, onto a fresh machine with nothing linked
//! too. The logic lives in `patchpilot_local::backup`.

use std::path::PathBuf;
use std::sync::Mutex;

use patchpilot_local::backup::{
    self, BackupReport, BackupSet, BackupSettings, RestoreReport, backup_dir,
};
use patchpilot_local::bring::FileToBring;
use patchpilot_local::transfer::{self, CloudFolder};
use patchpilot_local::vault::VaultSettings;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Listener, Manager, State};

use crate::rig::{RIG_CHANGED, Scanning, db_path, err, open};
use crate::sync::{app_dir, this_machine};

/// Emitted after a backup or a change of settings, for the UI.
pub const BACKUP_CHANGED: &str = "backup-changed";

/// The last backup's report, and whether one is running.
#[derive(Default)]
struct BackupState {
    last: Option<BackupReport>,
    error: Option<String>,
    running: bool,
}

#[derive(Default)]
struct Backups(Mutex<BackupState>);

/// Mirrored in `src/api.ts`.
#[derive(Debug, Serialize)]
pub struct BackupView {
    pub clouds: Vec<CloudFolder>,
    pub settings: BackupSettings,
    /// The `Patch Pilot Backup` folder in use, if the folder picked is there.
    pub dir: Option<String>,
    /// `owner/name` while the backup folder is a GitHub repo's copy.
    pub github: Option<String>,
    /// How far a long GitHub sync has got, while one runs.
    pub github_progress: Option<patchpilot_local::github::Progress>,
    /// Why the last GitHub sync didn't finish; it tries again on its own.
    pub github_error: Option<String>,
    /// The big-file folder, offered first when picking.
    pub transfer_folder: Option<String>,
    /// Every machine's backups there, this machine's first.
    pub sets: Vec<BackupSet>,
    /// This machine's ID, to tell its own backups from others'.
    pub this_machine: Option<String>,
    pub last: Option<BackupReport>,
    /// What went wrong with the last automatic backup.
    pub error: Option<String>,
    pub running: bool,
}

/// Mirrored in `src/api.ts`.
#[derive(Debug, Serialize)]
pub struct RestoreView {
    pub machine_name: String,
    pub stamp: String,
    /// What this machine lacks or has a different copy of, sorted by app.
    pub files: Vec<FileToBring>,
}

/// Back up after every scan that changed something, when that's switched on.
pub fn setup(app: &AppHandle) {
    app.manage(Backups::default());
    let handle = app.clone();
    app.listen_any(RIG_CHANGED, move |_| {
        let handle = handle.clone();
        std::thread::spawn(move || auto_back_up(&handle));
    });
    // Catch up on changes made while Patch Pilot was closed.
    let handle = app.clone();
    std::thread::spawn(move || auto_back_up(&handle));
}

fn settings() -> Result<BackupSettings, String> {
    Ok(backup::load_settings(&app_dir()?))
}

fn vault_root() -> Option<PathBuf> {
    app_dir().ok().and_then(|d| VaultSettings::load(&d).root)
}

pub(crate) fn auto_back_up(app: &AppHandle) {
    if settings().is_ok_and(|s| s.backs_up_on_change()) {
        let _ = run(app);
    }
}

/// Back up now unless a backup is running; the result is kept for the UI.
fn run(app: &AppHandle) -> Result<BackupReport, String> {
    let state = app.state::<Backups>();
    {
        let mut s = state.0.lock().map_err(|e| e.to_string())?;
        if s.running {
            return Err("A backup is already running.".into());
        }
        s.running = true;
    }
    let result = back_up();
    if let Ok(mut s) = state.0.lock() {
        s.running = false;
        match &result {
            Ok(r) => {
                s.last = Some(r.clone());
                s.error = None;
            }
            Err(e) => s.error = Some(e.clone()),
        }
    }
    let _ = app.emit(BACKUP_CHANGED, ());
    // With GitHub, the backup goes on to the repo (decision #73): a first
    // upload can take a while, and the line shows how far it's got.
    if result.is_ok() && crate::github::in_use() {
        let handle = app.clone();
        std::thread::spawn(move || {
            let _ = crate::github::sync_now(&handle);
        });
    }
    result
}

fn back_up() -> Result<BackupReport, String> {
    let dir = backup_dir(&settings()?)
        .ok_or("The backup folder isn't there now (signed out, or not connected).")?;
    let db = db_path()?;
    if !db.exists() {
        return Err("Scan this machine first.".into());
    }
    let store = open(&db)?;
    backup::back_up(
        &store,
        &this_machine()?,
        vault_root().as_deref(),
        &dir,
        patchpilot_local::now(),
    )
}

/// `owner/name` while the backup folder is a GitHub repo's copy.
fn github_repo(settings: &BackupSettings) -> Result<Option<String>, String> {
    if settings.service.as_deref() != Some(patchpilot_local::github::SERVICE) {
        return Ok(None);
    }
    Ok(patchpilot_local::github::load_settings(&app_dir()?)
        .repo
        .map(|r| r.full_name()))
}

#[tauri::command]
pub async fn backup_view(app: AppHandle) -> Result<BackupView, String> {
    let settings = settings()?;
    let dir = backup_dir(&settings);
    let this = this_machine().ok();
    let sets = match &dir {
        Some(d) => {
            let (d, this) = (d.clone(), this.clone().unwrap_or_default());
            tauri::async_runtime::spawn_blocking(move || backup::list_backups(&d, &this))
                .await
                .map_err(|e| e.to_string())?
        }
        None => Vec::new(),
    };
    let state = app.state::<Backups>();
    let (last, error, running) = state
        .0
        .lock()
        .map(|s| (s.last.clone(), s.error.clone(), s.running))
        .unwrap_or_default();
    let github = github_repo(&settings)?;
    let (github_progress, github_error) = match github {
        Some(_) => crate::github::status(&app),
        None => (None, None),
    };
    Ok(BackupView {
        clouds: transfer::this_machines_clouds(),
        github,
        github_progress,
        github_error,
        transfer_folder: transfer::load_settings(&app_dir()?)
            .folder
            .map(|f| f.to_string_lossy().into_owned()),
        dir: dir.map(|d| d.to_string_lossy().into_owned()),
        settings,
        sets,
        this_machine: this,
        last,
        error,
        running,
    })
}

/// Back up to `folder` (a cloud folder from the list, or any folder for
/// `custom`), or nowhere. Automatic backups stay as they were.
#[tauri::command]
pub async fn set_backup_folder(
    app: AppHandle,
    service: Option<String>,
    folder: Option<String>,
) -> Result<BackupView, String> {
    let settings = BackupSettings {
        folder: service.as_ref().and(folder).map(Into::into),
        service,
        ..settings()?
    };
    backup::save_settings(&app_dir()?, &settings)?;
    // One cloud folder for everything: big files go through it too, unless
    // this machine already picked a folder for them.
    let mut transfers = transfer::load_settings(&app_dir()?);
    if transfers.folder.is_none() && settings.folder.is_some() {
        transfers.service = settings.service.clone();
        transfers.folder = settings.folder.clone();
        transfer::save_settings(&app_dir()?, &transfers)?;
        crate::transfer::rewatch(&app);
    }
    let _ = app.emit(BACKUP_CHANGED, ());
    backup_view(app).await
}

/// Back up after every scan that finds a change, or only by hand.
#[tauri::command]
pub async fn set_backup_auto(app: AppHandle, on: bool) -> Result<BackupView, String> {
    let settings = BackupSettings {
        auto: on,
        ..settings()?
    };
    backup::save_settings(&app_dir()?, &settings)?;
    if on {
        let handle = app.clone();
        tauri::async_runtime::spawn_blocking(move || auto_back_up(&handle))
            .await
            .map_err(|e| e.to_string())?;
    }
    backup_view(app).await
}

#[tauri::command]
pub async fn back_up_now(app: AppHandle) -> Result<BackupReport, String> {
    tauri::async_runtime::spawn_blocking(move || run(&app))
        .await
        .map_err(|e| e.to_string())?
}

/// What restoring the backup `stamp` from the machine folder `folder` would
/// put on this machine.
#[tauri::command]
pub async fn backup_restore_list(folder: String, stamp: String) -> Result<RestoreView, String> {
    let dir = backup_dir(&settings()?).ok_or("Pick the folder your backups are in first.")?;
    let b = backup::load_backup(&dir, &folder, &stamp)?;
    let (profile, machine) = match this_machine() {
        Ok(this) => {
            let store = open(&db_path()?)?;
            (
                backup::restore_profile(&store, &this)?,
                store.latest_manifest(&this).map_err(err)?,
            )
        }
        Err(_) => (Default::default(), None),
    };
    Ok(RestoreView {
        files: backup::restore_list(&b, &profile, machine.as_ref(), vault_root().as_deref()),
        machine_name: b.machine_name,
        stamp: b.stamp,
    })
}

/// Restore the chosen files (`root/rel` keys as listed) and every file they
/// need, then rescan.
#[tauri::command]
pub async fn restore_backup(
    app: AppHandle,
    scanning: State<'_, Scanning>,
    folder: String,
    stamp: String,
    keys: Vec<String>,
) -> Result<RestoreReport, String> {
    let dir = backup_dir(&settings()?).ok_or("Pick the folder your backups are in first.")?;
    let this = this_machine()?;
    let blobs = app_dir()?.join("blobs");
    let report = tauri::async_runtime::spawn_blocking(move || {
        let b = backup::load_backup(&dir, &folder, &stamp)?;
        let mut store = open(&db_path()?)?;
        backup::restore(
            &mut store,
            &blobs,
            &this,
            &b,
            &keys,
            vault_root().as_deref(),
            patchpilot_local::now(),
        )
    })
    .await
    .map_err(|e| e.to_string())??;

    // Rescan so Compare and the lists show the restored files at once.
    if (report.written > 0 || report.same > 0) && scanning.start() {
        let rescanned = tauri::async_runtime::spawn_blocking({
            let app = app.clone();
            move || crate::rig::rescan_quietly(&app)
        })
        .await;
        scanning.finish();
        rescanned.map_err(|e| e.to_string())??;
        let _ = app.emit("data-changed", ());
    }
    Ok(report)
}
