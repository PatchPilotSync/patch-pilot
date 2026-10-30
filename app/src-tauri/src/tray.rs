//! Tray / menu bar icon and its popover (decision #35). Left click toggles a
//! small borderless window next to the icon; it hides again when it loses
//! focus. Double-clicking the icon (Windows) opens the main window. Closing the main window hides it, so Patch Pilot keeps living in the
//! tray until the user picks Quit from the icon's menu. On macOS the Dock icon
//! only shows while the main window is open (decision #39).

use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, Rect, WindowEvent};
use tauri_plugin_autostart::ManagerExt;

pub const MAIN: &str = "main";
pub const POPOVER: &str = "tray";
/// The tray icon's ID.
pub const ID: &str = "patchpilot";

/// Gap between the tray icon and the popover, in logical pixels.
const GAP: f64 = 6.0;

/// When the popover last hid. Clicking the icon while the popover is open
/// first blurs (hides) it; without this the same click would open it again.
#[derive(Default)]
pub struct LastHidden(Mutex<Option<Instant>>);

#[cfg(target_os = "macos")]
const ICON: &[u8] = include_bytes!("../icons/tray-template@2x.png");
#[cfg(not(target_os = "macos"))]
const ICON: &[u8] = include_bytes!("../icons/tray-white@2x.png");

pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Patch Pilot", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
    // Temporary, for testing onboarding: shows the welcome screen and tour again
    // without touching any saved data. Remove before a public launch.
    let replay = MenuItem::with_id(app, "replay", "Replay First Run", true, None::<&str>)?;
    let updates = MenuItem::with_id(app, "updates", "Check for Updates…", true, None::<&str>)?;
    // Shows the real state, so a change made in Task Manager or macOS Login
    // Items is reflected; refreshed whenever it's clicked.
    let at_login = CheckMenuItem::with_id(
        app,
        "login",
        "Open at Login",
        true,
        app.autolaunch().is_enabled().unwrap_or(false),
        None::<&str>,
    )?;
    // Quiet OS notifications when a machine drifts (M9).
    let alerts = CheckMenuItem::with_id(
        app,
        "alerts",
        "Drift Alerts",
        true,
        crate::alerts::enabled(),
        None::<&str>,
    )?;
    // Keep files in sync (decision #70) through the cloud folder.
    let sync = CheckMenuItem::with_id(
        app,
        "sync",
        "Keep Files in Sync",
        true,
        crate::autosync::enabled(),
        None::<&str>,
    )?;
    app.manage(Checks {
        at_login: at_login.clone(),
        alerts: alerts.clone(),
        sync: sync.clone(),
    });
    let quit = MenuItem::with_id(app, "quit", "Quit Patch Pilot", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &open, &settings, &updates, &replay, &at_login, &alerts, &sync, &quit,
        ],
    )?;

    TrayIconBuilder::with_id(ID)
        .icon(Image::from_bytes(ICON)?)
        .icon_as_template(cfg!(target_os = "macos"))
        .tooltip("Patch Pilot")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id.as_ref() {
            "open" => show_main(app),
            "settings" => {
                show_main(app);
                let _ = app.emit_to(MAIN, "open-settings", ());
            }
            "replay" => {
                show_main(app);
                let _ = app.emit_to(MAIN, "replay-first-run", ());
            }
            // The main window's update bar does the check and shows the answer.
            "updates" => {
                show_main(app);
                let _ = app.emit_to(MAIN, "check-for-updates", ());
            }
            "login" => {
                // The menu has already flipped the tick; make the OS match it.
                let autolaunch = app.autolaunch();
                let _ = if at_login.is_checked().unwrap_or(false) {
                    autolaunch.enable()
                } else {
                    autolaunch.disable()
                };
                let _ = at_login.set_checked(autolaunch.is_enabled().unwrap_or(false));
                let _ = app.emit_to(MAIN, crate::settings::SETTINGS_CHANGED, ());
            }
            "alerts" => {
                crate::alerts::set_enabled(alerts.is_checked().unwrap_or(true));
                let _ = alerts.set_checked(crate::alerts::enabled());
                let _ = app.emit_to(MAIN, crate::settings::SETTINGS_CHANGED, ());
            }
            "sync" => {
                let on = sync.is_checked().unwrap_or(false);
                if crate::autosync::set_enabled(app, on).is_err() {
                    // No cloud folder yet: pick one in the app.
                    show_main(app);
                    let _ = app.emit_to(MAIN, "set-up-sync", ());
                }
                refresh_sync_check(app);
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| match event {
            TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                rect,
                ..
            } => toggle_popover(tray.app_handle(), rect),
            // Windows only; macOS menu bar items don't report double-clicks.
            TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            } => show_main(tray.app_handle()),
            _ => {}
        })
        .build(app)?;

    if let Some(popover) = app.get_webview_window(POPOVER) {
        let handle = app.clone();
        let window = popover.clone();
        popover.on_window_event(move |event| match event {
            WindowEvent::Focused(false) => hide_popover(&handle),
            WindowEvent::ScaleFactorChanged { .. } => realign_soon(&window),
            _ => {}
        });
    }
    if let Some(main) = app.get_webview_window(MAIN) {
        let window = main.clone();
        main.on_window_event(move |event| match event {
            WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                let _ = window.hide();
                set_dock_icon(window.app_handle(), false);
            }
            // Dragged to a screen with another scale.
            WindowEvent::ScaleFactorChanged { .. } => realign_soon(&window),
            _ => {}
        });
    }
    Ok(())
}

/// The tray menu's ticked items, to keep them current when Settings changes them.
struct Checks {
    at_login: CheckMenuItem<tauri::Wry>,
    alerts: CheckMenuItem<tauri::Wry>,
    sync: CheckMenuItem<tauri::Wry>,
}

/// Make the tray's "Keep Files in Sync" tick match the setting.
pub fn refresh_sync_check(app: &AppHandle) {
    if let Some(checks) = app.try_state::<Checks>() {
        let _ = checks.sync.set_checked(crate::autosync::enabled());
    }
}

/// Make the tray's Open at Login and Drift Alerts ticks match the settings.
pub fn refresh_checks(app: &AppHandle) {
    if let Some(checks) = app.try_state::<Checks>() {
        let _ = checks
            .at_login
            .set_checked(app.autolaunch().is_enabled().unwrap_or(false));
        let _ = checks.alerts.set_checked(crate::alerts::enabled());
    }
}

fn toggle_popover(app: &AppHandle, icon: Rect) {
    let Some(popover) = app.get_webview_window(POPOVER) else {
        return;
    };
    if popover.is_visible().unwrap_or(false) {
        hide_popover(app);
        return;
    }
    let just_hidden = app
        .state::<LastHidden>()
        .0
        .lock()
        .ok()
        .and_then(|t| *t)
        .is_some_and(|t| t.elapsed() < Duration::from_millis(250));
    if just_hidden {
        return;
    }
    if let Some(at) = popover_position(app, &popover, icon) {
        let _ = popover.set_position(at);
    }
    realign_soon(&popover);
    let _ = popover.show();
    let _ = popover.set_focus();
    // The popover refreshes itself when it appears; no polling while hidden.
    let _ = popover.emit_to(POPOVER, "popover-shown", ());
}

fn hide_popover(app: &AppHandle) {
    if let Some(popover) = app.get_webview_window(POPOVER)
        && popover.is_visible().unwrap_or(false)
    {
        let _ = popover.hide();
        if let Ok(mut t) = app.state::<LastHidden>().0.lock() {
            *t = Some(Instant::now());
        }
    }
}

/// Centred on the icon, below it when the icon is in the top half of its
/// screen (macOS menu bar) and above it otherwise (Windows taskbar), kept
/// inside the screen.
fn popover_position(
    app: &AppHandle,
    popover: &tauri::WebviewWindow,
    icon: Rect,
) -> Option<PhysicalPosition<i32>> {
    let scale = popover.scale_factor().ok()?;
    let at = icon.position.to_physical::<f64>(scale);
    let size = icon.size.to_physical::<f64>(scale);
    let win = popover.outer_size().ok()?;
    let (w, h) = (f64::from(win.width), f64::from(win.height));
    let gap = GAP * scale;

    let monitor = app.monitor_from_point(at.x, at.y).ok().flatten()?;
    let (mx, my) = (
        f64::from(monitor.position().x),
        f64::from(monitor.position().y),
    );
    let (mw, mh) = (
        f64::from(monitor.size().width),
        f64::from(monitor.size().height),
    );

    let x = (at.x + size.width / 2.0 - w / 2.0).clamp(mx + gap, (mx + mw - w - gap).max(mx));
    let y = if at.y + size.height / 2.0 < my + mh / 2.0 {
        at.y + size.height + gap
    } else {
        at.y - h - gap
    };
    Some(PhysicalPosition::new(x.round() as i32, y.round() as i32))
}

pub fn show_main(app: &AppHandle) {
    hide_popover(app);
    if let Some(main) = app.get_webview_window(MAIN) {
        set_dock_icon(app, true);
        let _ = main.unminimize();
        realign_soon(&main);
        let _ = main.show();
        let _ = main.set_focus();
    }
}

/// Keep WebView2's drawing in place (decision #115): now, before the window
/// shows, and once more after, in case WebView2 rebuilt it meanwhile.
fn realign_soon(window: &tauri::WebviewWindow) {
    if !cfg!(windows) {
        return;
    }
    crate::realign::realign(window);
    let window = window.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(500));
        crate::realign::realign(&window);
    });
}

/// macOS: Patch Pilot is a menu bar app with a Dock icon only while its main
/// window is open, like other always-on utilities.
pub fn set_dock_icon(_app: &AppHandle, _visible: bool) {
    #[cfg(target_os = "macos")]
    let _ = _app.set_dock_visibility(_visible);
}

/// The popover's Expand and Get Up To Speed buttons, row clicks and double-clicks. `machine` opens Compare on
/// that machine; `plan` goes straight to its Up To Speed checklist.
#[tauri::command]
pub fn open_main_window(app: AppHandle, machine: Option<String>, plan: bool) {
    show_main(&app);
    if let Some(id) = machine {
        let _ = app.emit_to(MAIN, "show-machine", ShowMachine { id, plan });
    }
}

#[derive(Clone, serde::Serialize)]
struct ShowMachine {
    id: String,
    plan: bool,
}
