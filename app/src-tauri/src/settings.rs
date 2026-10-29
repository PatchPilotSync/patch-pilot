//! Settings (decision #96): the switches that also sit in the tray menu (Open
//! at Login, Drift Alerts) plus how the app looks, in one place. Text size is
//! the main window's zoom, so every size in the layout grows together; the
//! theme goes to both windows and their title bars.

use chrono::Timelike;
use patchpilot_local::app_prefs::{self, AppPrefs};
use patchpilot_local::ignore::Daw;
use patchpilot_local::ui_prefs::{self, Theme, UiPrefs};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_autostart::ManagerExt;

use crate::sync::app_dir;
use crate::tray::{MAIN, POPOVER};

/// Sent to the main window when the tray menu changed a switch Settings shows.
pub const SETTINGS_CHANGED: &str = "settings-changed";

/// Sent to both windows when the look changes, with the new [`SettingsView`].
pub const UI_CHANGED: &str = "ui-changed";

/// Mirrored by `SettingsView` in `src/api.ts`.
#[derive(Debug, Clone, Serialize)]
pub struct SettingsView {
    pub open_at_login: bool,
    pub drift_alerts: bool,
    pub text_size: f64,
    pub theme: Theme,
    pub text_sizes: [f64; 4],
    pub compact: bool,
    /// Rescans, notifications, extra folders, restore points (decision #97).
    pub prefs: AppPrefs,
    /// Whether this machine watches Reaper and Ableton Live; both until scanned.
    pub reaper: bool,
    pub live: bool,
    /// Where Patch Pilot keeps its data on this machine.
    pub data_dir: String,
}

fn view(app: &AppHandle) -> SettingsView {
    let ui = prefs();
    let dir = app_dir().unwrap_or_default();
    let watched = |daw| {
        daws_store().is_none_or(|(store, id)| {
            patchpilot_local::ignore::daw_watched(&store, &id, daw).unwrap_or(true)
        })
    };
    SettingsView {
        open_at_login: app.autolaunch().is_enabled().unwrap_or(false),
        drift_alerts: crate::alerts::enabled(),
        text_size: ui.text_size,
        theme: ui.theme,
        text_sizes: ui_prefs::TEXT_SIZES,
        compact: ui.compact,
        prefs: app_prefs::load(&dir),
        reaper: watched(Daw::Reaper),
        live: watched(Daw::Live),
        data_dir: dir.to_string_lossy().into_owned(),
    }
}

/// The store and this machine's ID, once it has been scanned.
fn daws_store() -> Option<(patchpilot_store::Store, String)> {
    let db = crate::rig::db_path().ok().filter(|d| d.exists())?;
    let id = patchpilot_store::read_machine_id(patchpilot_local::app_dir(&db)).ok()??;
    Some((crate::rig::open(&db).ok()?, id))
}

/// This machine's app preferences (decision #97).
pub fn app_prefs() -> AppPrefs {
    app_dir().map(|d| app_prefs::load(&d)).unwrap_or_default()
}

/// Whether an OS notification may show now: outside the quiet hours.
pub fn may_notify() -> bool {
    let hour = chrono::Local::now().hour();
    app_prefs().may_notify(u8::try_from(hour).unwrap_or(0))
}

fn prefs() -> UiPrefs {
    app_dir().map(|d| ui_prefs::load(&d)).unwrap_or_default()
}

/// Give the windows this machine's text size and theme; at launch and after each change.
pub fn apply(app: &AppHandle) {
    let prefs = prefs();
    let theme = match prefs.theme {
        Theme::System => None,
        Theme::Light => Some(tauri::Theme::Light),
        Theme::Dark => Some(tauri::Theme::Dark),
    };
    if let Some(main) = app.get_webview_window(MAIN) {
        let _ = main.set_zoom(prefs.text_size);
        let _ = main.set_theme(theme);
    }
    // The popover keeps its size: it's a fixed little window by the tray icon.
    if let Some(popover) = app.get_webview_window(POPOVER) {
        let _ = popover.set_theme(theme);
    }
}

#[tauri::command]
pub fn settings_view(app: AppHandle) -> SettingsView {
    view(&app)
}

#[tauri::command]
pub fn set_open_at_login(app: AppHandle, on: bool) -> Result<SettingsView, String> {
    let autolaunch = app.autolaunch();
    if on {
        autolaunch.enable()
    } else {
        autolaunch.disable()
    }
    .map_err(|e| e.to_string())?;
    crate::tray::refresh_checks(&app);
    Ok(view(&app))
}

#[tauri::command]
pub fn set_drift_alerts(app: AppHandle, on: bool) -> SettingsView {
    crate::alerts::set_enabled(on);
    crate::tray::refresh_checks(&app);
    view(&app)
}

#[tauri::command]
pub fn set_text_size(app: AppHandle, size: f64) -> Result<SettingsView, String> {
    ui_prefs::set_text_size(&app_dir()?, size)?;
    changed(&app)
}

#[tauri::command]
pub fn set_theme(app: AppHandle, theme: Theme) -> Result<SettingsView, String> {
    ui_prefs::set_theme(&app_dir()?, theme)?;
    changed(&app)
}

#[tauri::command]
pub fn set_compact(app: AppHandle, compact: bool) -> Result<SettingsView, String> {
    ui_prefs::set_compact(&app_dir()?, compact)?;
    changed(&app)
}

/// Save the app preferences Settings shows, then let the watcher and the
/// restore point clean-up follow them.
#[tauri::command]
pub fn set_app_prefs(app: AppHandle, prefs: AppPrefs) -> Result<SettingsView, String> {
    app_prefs::save(&app_dir()?, &prefs)?;
    crate::background::prefs_changed(&app);
    let handle = app.clone();
    std::thread::spawn(move || prune_restore_points(&handle));
    Ok(view(&app))
}

/// Watch a DAW on this machine or not; a rescan then adds or drops its files.
#[tauri::command]
pub fn set_daw_watched(app: AppHandle, daw: Daw, on: bool) -> Result<SettingsView, String> {
    let (mut store, id) = daws_store().ok_or("scan this machine first")?;
    patchpilot_local::ignore::set_daw_watched(&mut store, &id, daw, on, patchpilot_local::now())?;
    crate::sync::nudge(&app);
    crate::background::rescan_soon(&app);
    let _ = app.emit("data-changed", ());
    Ok(view(&app))
}

/// Show Patch Pilot's data folder in Finder or Explorer.
#[tauri::command]
pub fn open_data_folder() -> Result<(), String> {
    let db = crate::rig::db_path()?;
    if db.exists() {
        patchpilot_local::open::reveal(&db).map_err(|e| e.to_string())
    } else {
        patchpilot_local::open::reveal(&app_dir()?).map_err(|e| e.to_string())
    }
}

/// Start over on this machine at the next launch; the window then restarts the app.
#[tauri::command]
pub fn reset_this_machine() -> Result<(), String> {
    patchpilot_local::reset::request(&app_dir()?)
}

/// At launch, before anything opens the store: finish a reset asked for.
pub fn finish_reset() {
    if let Ok(db) = crate::rig::db_path() {
        let stamp = chrono::Local::now().format("%Y-%m-%d %H%M").to_string();
        let _ = patchpilot_local::reset::finish(&db, &stamp);
    }
}

/// Forget restore points older than Settings keeps, and the saved copies only
/// they needed. At launch and when the setting changes.
pub fn prune_restore_points(_app: &AppHandle) {
    let Some(days) = app_prefs().restore_days else {
        return;
    };
    let Some((mut store, id)) = daws_store() else {
        return;
    };
    let before = patchpilot_local::now() - i64::from(days) * 86_400;
    if store.prune_restore_points(&id, before).unwrap_or(0) == 0 {
        return;
    }
    let (Ok(keep), Ok(dir)) = (store.restore_blobs(), app_dir()) else {
        return;
    };
    // A day's grace, for a copy saved just before its restore point is recorded.
    patchpilot_store::BlobStore::new(dir.join("blobs"))
        .remove_unused(&keep, std::time::Duration::from_secs(86_400));
}

fn changed(app: &AppHandle) -> Result<SettingsView, String> {
    apply(app);
    let view = view(app);
    let _ = app.emit(UI_CHANGED, view.clone());
    Ok(view)
}
