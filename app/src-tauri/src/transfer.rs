//! Big-file hand-off (decision #64): Bring over files too big for the relay
//! go through `Patch Pilot Transfers` in a cloud folder each machine picks.
//! This machine sends what others need when the user says so, and places
//! what it needs on its own as it arrives (watched, not polled), then cleans
//! up. The logic lives in `patchpilot_local::transfer`.

use std::sync::Mutex;
use std::time::Duration;

use patchpilot_local::FolderWatch;
use patchpilot_local::transfer::{
    self, CloudFolder, ReceiveReport, Sendable, TransferSettings, transfer_dir,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Listener, Manager};

use crate::rig::{db_path, open};
use crate::sync::{app_dir, this_machine};

/// Cloud apps write a file in pieces; wait for them to settle.
const QUIET: Duration = Duration::from_secs(5);
/// Emitted after this machine placed or sent files, for the UI.
pub const TRANSFERS_CHANGED: &str = "transfers-changed";

#[derive(Default)]
struct Watch(Mutex<Option<FolderWatch>>);

/// Mirrored in `src/api.ts`.
#[derive(Debug, Serialize)]
pub struct TransferView {
    /// Cloud folders found on this machine, in list order.
    pub clouds: Vec<CloudFolder>,
    pub settings: TransferSettings,
    /// The `Patch Pilot Transfers` folder in use, if the folder picked is there.
    pub dir: Option<String>,
    /// Big files linked machines need that this machine can send.
    pub sendable: Vec<Sendable>,
    /// Big files this machine needs (each sent from a machine that has it).
    pub needed: usize,
    /// Big files this machine asked for that haven't arrived yet.
    pub requested: usize,
    /// What the last look at the folder did here.
    pub last: ReceiveReport,
}

/// Watch the transfer folder, look in it now, and again whenever a linked
/// machine's scan arrives (it may be what made a file wanted).
pub fn setup(app: &AppHandle) {
    app.manage(Watch::default());
    app.manage(Mutex::new(ReceiveReport::default()));
    rewatch(app);
    let handle = app.clone();
    std::thread::spawn(move || receive_now(&handle));
    let handle = app.clone();
    app.listen_any("data-changed", move |_| {
        let handle = handle.clone();
        std::thread::spawn(move || receive_now(&handle));
    });
}

fn settings() -> Result<TransferSettings, String> {
    Ok(transfer::load_settings(&app_dir()?))
}

pub(crate) fn rewatch(app: &AppHandle) {
    let dir = settings().ok().as_ref().and_then(transfer_dir);
    let state = app.state::<Watch>();
    let Ok(mut slot) = state.0.lock() else {
        return;
    };
    *slot = None;
    let Some(dir) = dir else {
        return;
    };
    let _ = std::fs::create_dir_all(&dir);
    let handle = app.clone();
    *slot = patchpilot_local::watch_folder_flat(dir, QUIET, move || receive_now(&handle)).ok();
}

/// Send what other machines asked for, place what's arrived and clean up.
/// Errors are kept quiet: the next change or scan tries again.
fn receive_now(app: &AppHandle) {
    let Ok(report) = receive() else {
        return;
    };
    if let Ok(mut last) = app.state::<Mutex<ReceiveReport>>().lock() {
        *last = report.clone();
    }
    // The preset and template folders are watched, so a rescan follows the
    // writes on its own.
    if report.placed > 0 || report.cleaned > 0 {
        let _ = app.emit(TRANSFERS_CHANGED, &report);
    }
}

fn receive() -> Result<ReceiveReport, String> {
    let Some(dir) = transfer_dir(&settings()?) else {
        return Ok(ReceiveReport::default());
    };
    let db = db_path()?;
    if !db.exists() || !dir.is_dir() {
        return Ok(ReceiveReport::default());
    }
    let this = this_machine()?;
    let blobs = app_dir()?.join("blobs");
    let mut store = open(&db)?;
    transfer::send_requested(&store, &this, &dir)?;
    transfer::receive(&mut store, &blobs, &this, &dir, patchpilot_local::now())
}

/// The cloud folder big files go through here, as people call it
/// ("Dropbox"), when one is picked and there.
pub(crate) fn cloud_name() -> Option<String> {
    let settings = settings().ok()?;
    let folder = settings.folder.clone()?;
    transfer_dir(&settings)?;
    let name = transfer::this_machines_clouds()
        .into_iter()
        .find(|c| c.path == folder)
        .map(|c| c.name);
    Some(name.unwrap_or_else(|| {
        folder.file_name().map_or_else(
            || "your cloud folder".to_owned(),
            |n| n.to_string_lossy().into_owned(),
        )
    }))
}

/// Ask the other machines for `files` (too big for the link) through the
/// cloud folder. False when no cloud folder is picked here.
pub(crate) fn request_big(
    this: &str,
    files: &[patchpilot_local::bring::FileToBring],
) -> Result<bool, String> {
    if files.is_empty() {
        return Ok(true);
    }
    let Some(dir) = transfer_dir(&settings()?) else {
        return Ok(false);
    };
    let name = open(&db_path()?)?
        .machine(this)
        .map(|m| m.name)
        .unwrap_or_else(|_| this.to_owned());
    transfer::request(&dir, this, &name, files)?;
    Ok(true)
}

#[tauri::command]
pub async fn transfer_view(app: AppHandle) -> Result<TransferView, String> {
    let settings = settings()?;
    let dir = transfer_dir(&settings);
    let db = db_path()?;
    let (sendable, needed, requested) = match (db.exists(), this_machine()) {
        (true, Ok(this)) => {
            let store = open(&db)?;
            (
                transfer::sendable(&store, &this, dir.as_deref())?,
                transfer::needed_count(&store, &this)?,
                dir.as_deref()
                    .map_or(0, |d| transfer::requested_count(d, &this)),
            )
        }
        _ => (Vec::new(), 0, 0),
    };
    let last = app
        .state::<Mutex<ReceiveReport>>()
        .lock()
        .map(|r| r.clone())
        .unwrap_or_default();
    Ok(TransferView {
        clouds: transfer::this_machines_clouds(),
        dir: dir.map(|d| d.to_string_lossy().into_owned()),
        settings,
        sendable,
        needed,
        requested,
        last,
    })
}

/// Use `folder` (a cloud folder from the list, or any folder for
/// `custom`), or nothing, for big files on this machine.
#[tauri::command]
pub async fn set_transfer_folder(
    app: AppHandle,
    service: Option<String>,
    folder: Option<String>,
) -> Result<TransferView, String> {
    let settings = TransferSettings {
        folder: service.as_ref().and(folder).map(Into::into),
        service,
    };
    transfer::save_settings(&app_dir()?, &settings)?;
    rewatch(&app);
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || receive_now(&handle))
        .await
        .map_err(|e| e.to_string())?;
    transfer_view(app).await
}

/// Put every big file linked machines need into the transfer folder.
#[tauri::command]
pub async fn send_big_files(app: AppHandle) -> Result<usize, String> {
    let dir = transfer_dir(&settings()?)
        .ok_or("Pick a cloud folder for big files first, on this machine and the other one.")?;
    let this = this_machine()?;
    let files = transfer::sendable(&open(&db_path()?)?, &this, Some(&dir))?;
    let sent = tauri::async_runtime::spawn_blocking(move || transfer::send(&dir, &files))
        .await
        .map_err(|e| e.to_string())??;
    let _ = app.emit(TRANSFERS_CHANGED, ());
    Ok(sent)
}
