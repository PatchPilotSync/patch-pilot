//! Drive space (decision #101): this machine's drives, measured when the UI
//! looks and after scans, and sent to linked machines when they moved. What
//! is measured and how lives in `patchpilot_local::disks`.

use patchpilot_local::vault::VaultSettings;
use tauri::AppHandle;

use crate::rig::{db_path, open};

/// Measure this machine's drives and keep the reading if it moved since the
/// last one (a gigabyte, or a day), so linked machines see it. A few cheap
/// OS calls, run before the UI reads machines; quiet on any failure.
pub fn refresh(app: &AppHandle) {
    let Ok(db) = db_path() else {
        return;
    };
    let dir = patchpilot_local::app_dir(&db);
    let Ok(Some(this)) = patchpilot_store::read_machine_id(dir) else {
        return;
    };
    if !db.exists() {
        return;
    }
    let Ok(mut store) = open(&db) else {
        return;
    };
    // No scan yet: nothing to show it on.
    let Ok(machine) = store.machine(&this) else {
        return;
    };
    let now = patchpilot_local::disks::this_machine(&machine, vault().as_deref());
    if !now.is_empty()
        && patchpilot_local::disks::moved(&machine.disks, &now)
        && store.set_disks(&this, &now).unwrap_or(false)
    {
        crate::sync::nudge(app);
    }
}

/// The Installer Vault folder, measured with the other drives.
pub fn vault() -> Option<std::path::PathBuf> {
    let db = db_path().ok()?;
    VaultSettings::load(patchpilot_local::app_dir(&db)).root
}
