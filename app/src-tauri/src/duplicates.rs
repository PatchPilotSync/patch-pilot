//! Duplicate presets (decision #66): find the same preset saved in several
//! folders on this machine and remove the extra copies as one restore point
//! History can undo. The search and the clean-up live in
//! `patchpilot_local::duplicates`.

use patchpilot_local::duplicates::{self, Choice, CleanUp, Duplicates};
use tauri::{AppHandle, Emitter, State};

use crate::rig::{Scanning, db_path, open, rescan_quietly};

/// Look for duplicate presets. Reads file sizes, then hashes only files that
/// share a size, so it runs off the UI thread.
#[tauri::command]
pub async fn find_duplicates() -> Result<Duplicates, String> {
    tauri::async_runtime::spawn_blocking(|| {
        duplicates::find_duplicates(&duplicates::standard_places())
    })
    .await
    .map_err(|e| e.to_string())
}

/// Remove the chosen copies, then rescan so preset folders and Compare
/// catch up.
#[tauri::command]
pub async fn clean_up_duplicates(
    app: AppHandle,
    scanning: State<'_, Scanning>,
    choices: Vec<Choice>,
) -> Result<CleanUp, String> {
    let db = db_path()?;
    let id = patchpilot_store::read_machine_id(patchpilot_local::app_dir(&db))
        .map_err(|e| e.to_string())?
        .ok_or("Scan this machine first.")?;
    let blobs = patchpilot_local::restore::default_blob_dir().ok_or("no app-data folder")?;
    let done = tauri::async_runtime::spawn_blocking(move || {
        let mut store = open(&db)?;
        duplicates::clean_up(
            &mut store,
            &blobs,
            &id,
            &duplicates::standard_places(),
            &choices,
            patchpilot_local::now(),
        )
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;
    if done.removed > 0 && scanning.start() {
        let rescanned = tauri::async_runtime::spawn_blocking({
            let app = app.clone();
            move || rescan_quietly(&app)
        })
        .await;
        scanning.finish();
        rescanned.map_err(|e| e.to_string())??;
        let _ = app.emit("data-changed", ());
    }
    Ok(done)
}
