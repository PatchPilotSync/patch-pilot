//! Settings (decision #96): the switches that also sit in the tray menu (Open
//! at Login, Drift Alerts) plus how the app looks, in one place. Text size is
//! the main window's zoom, so every size in the layout grows together; the
//! theme goes to both windows and their title bars.

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
}

fn view(app: &AppHandle) -> SettingsView {
    let prefs = prefs();
    SettingsView {
        open_at_login: app.autolaunch().is_enabled().unwrap_or(false),
        drift_alerts: crate::alerts::enabled(),
        text_size: prefs.text_size,
        theme: prefs.theme,
        text_sizes: ui_prefs::TEXT_SIZES,
    }
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

fn changed(app: &AppHandle) -> Result<SettingsView, String> {
    apply(app);
    let view = view(app);
    let _ = app.emit(UI_CHANGED, view.clone());
    Ok(view)
}
