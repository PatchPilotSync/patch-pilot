//! Drift alerts (M9): a quiet OS notification when a machine falls further
//! from the Golden Rig, or when this machine gets something the Golden Rig
//! doesn't have. Checked only after something changed (a scan here, data from
//! a linked machine), never on a timer. "Drift Alerts" in the tray menu turns
//! them off. What to say lives in `patchpilot_local::golden`.

use std::sync::Mutex;

use patchpilot_store::Store;
use tauri::{AppHandle, Listener, Manager};
use tauri_plugin_notification::NotificationExt;

use crate::rig::{RIG_CHANGED, db_path, open};

/// One check at a time, so two events in a row can't both alert.
#[derive(Default)]
struct Checking(Mutex<()>);

pub fn setup(app: &AppHandle) {
    app.manage(Checking::default());
    for event in [RIG_CHANGED, "data-changed"] {
        let handle = app.clone();
        app.listen_any(event, move |_| {
            let handle = handle.clone();
            std::thread::spawn(move || check(&handle));
        });
    }
}

fn check(app: &AppHandle) {
    let state = app.state::<Checking>();
    let Ok(_guard) = state.0.lock() else {
        return;
    };
    let Ok(db) = db_path() else {
        return;
    };
    if !db.exists() {
        return;
    }
    let Ok(store) = open(&db) else {
        return;
    };
    let dir = patchpilot_local::app_dir(&db);
    let this = patchpilot_store::read_machine_id(dir).ok().flatten();
    let Ok(alerts) = patchpilot_local::golden::check_alerts(&store, dir, this.as_deref()) else {
        return;
    };
    for alert in alerts {
        // Best effort: a missing notification permission shouldn't break anything.
        let _ = app
            .notification()
            .builder()
            .title(alert.title())
            .body(alert.body())
            .show();
    }
}

/// Take the current state as told, after a change made on this machine.
pub fn rebaseline(app: &AppHandle, store: &Store) {
    let state = app.state::<Checking>();
    let Ok(_guard) = state.0.lock() else {
        return;
    };
    let Ok(db) = db_path() else {
        return;
    };
    let dir = patchpilot_local::app_dir(&db);
    let this = patchpilot_store::read_machine_id(dir).ok().flatten();
    let _ = patchpilot_local::golden::rebaseline(store, dir, this.as_deref());
}

pub fn enabled() -> bool {
    db_path()
        .is_ok_and(|db| patchpilot_local::golden::alerts_enabled(patchpilot_local::app_dir(&db)))
}

pub fn set_enabled(on: bool) {
    if let Ok(db) = db_path() {
        let _ = patchpilot_local::golden::set_alerts_enabled(patchpilot_local::app_dir(&db), on);
    }
}
