//! Reaper shortcuts on a Mac (decision #81): the setting, and moving this
//! Mac's key map when it changes. The logic is `patchpilot_local::mac_keys`.

use patchpilot_core::Os;
use patchpilot_local::mac_keys::{self, MacKeysView};
use patchpilot_store::Store;
use tauri::{AppHandle, Emitter};

use crate::rig::{db_path, open};
use crate::sync::{app_dir, this_machine};

/// Before the first scan: which form this machine's key map is in.
pub fn setup(_app: &AppHandle) {
    if let Ok(dir) = app_dir() {
        mac_keys::load(&dir);
    }
}

fn os() -> Os {
    Os::current().unwrap_or(Os::Windows)
}

/// Move this machine's key map to match the setting, if it doesn't yet.
/// `Ok(true)` while that waits for Reaper to close.
pub fn settle(store: &mut Store) -> Result<bool, String> {
    let dir = app_dir()?;
    mac_keys::settle(
        store,
        &dir.join("blobs"),
        &this_machine()?,
        &dir,
        os(),
        patchpilot_local::now(),
    )
}

#[tauri::command]
pub fn mac_keys_view() -> Result<MacKeysView, String> {
    let db = db_path()?;
    let machine = match this_machine() {
        Ok(id) if db.exists() => open(&db)?.latest_manifest(&id).map_err(|e| e.to_string())?,
        _ => None,
    };
    Ok(mac_keys::view(&app_dir()?, machine.as_ref(), os()))
}

/// Turn moving shortcuts for macOS on or off here; the key map moves now if
/// Reaper is closed, else when it next is (the sync look tries again).
#[tauri::command]
pub async fn set_mac_keys(app: AppHandle, on: bool) -> Result<MacKeysView, String> {
    mac_keys::set(&app_dir()?, on)?;
    let db = db_path()?;
    if db.exists() && this_machine().is_ok() && !settle(&mut open(&db)?)? {
        let handle = app.clone();
        std::thread::spawn(move || {
            crate::backup::auto_back_up(&handle);
            crate::autosync::look(&handle);
        });
        let _ = app.emit("data-changed", ());
    }
    mac_keys_view()
}
