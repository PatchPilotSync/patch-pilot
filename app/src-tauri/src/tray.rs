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

/// Gap between the menu bar and the popover (macOS), in logical pixels.
const GAP: f64 = 6.0;
/// Gap between the popover and the taskbar and screen edge (Windows), in
/// logical pixels: what Windows 11's own flyouts leave.
const MARGIN: f64 = 12.0;

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

/// Where the popover goes, as its outer (window) position.
fn popover_position(
    app: &AppHandle,
    popover: &tauri::WebviewWindow,
    icon: Rect,
) -> Option<PhysicalPosition<i32>> {
    let icon_at = icon.position.to_physical::<f64>(1.0);
    let monitor = app
        .monitor_from_point(icon_at.x, icon_at.y)
        .ok()
        .flatten()?;
    let scale = monitor.scale_factor();
    let icon_size = icon.size.to_physical::<f64>(scale);

    // Sizes at the scale of the screen it's going to, which may not be the
    // one it was last on.
    let rescale = scale / popover.scale_factor().ok()?;
    let inner = popover.inner_size().ok()?;
    let (w, h) = (
        f64::from(inner.width) * rescale,
        f64::from(inner.height) * rescale,
    );
    // Windows counts invisible resize borders in the window's position.
    let (outer_at, inner_at) = (
        popover.outer_position().ok()?,
        popover.inner_position().ok()?,
    );
    let border = (
        f64::from(inner_at.x - outer_at.x) * rescale,
        f64::from(inner_at.y - outer_at.y) * rescale,
    );

    let screen = Area::new(monitor.position(), monitor.size());
    let work = Area::new(&monitor.work_area().position, &monitor.work_area().size);
    let icon_mid = (
        icon_at.x + icon_size.width / 2.0,
        icon_at.y + icon_size.height / 2.0,
    );
    let (x, y) = place(screen, work, icon_mid, (w, h), scale);
    Some(PhysicalPosition::new(
        (x - border.0).round() as i32,
        (y - border.1).round() as i32,
    ))
}

/// A screen rectangle in physical pixels.
#[derive(Clone, Copy, Debug)]
struct Area {
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
}

impl Area {
    fn new(at: &PhysicalPosition<i32>, size: &tauri::PhysicalSize<u32>) -> Self {
        let (left, top) = (f64::from(at.x), f64::from(at.y));
        Self {
            left,
            top,
            right: left + f64::from(size.width),
            bottom: top + f64::from(size.height),
        }
    }
}

/// Top-left corner for a popover of `size` (decision #119). Windows: in the
/// corner of the work area by the taskbar, like Windows' own flyouts,
/// wherever the icon is (the hidden-icons overflow puts it mid-screen).
/// macOS: under the menu bar icon, centred on it. Kept on the screen.
fn place(
    screen: Area,
    work: Area,
    icon_mid: (f64, f64),
    (w, h): (f64, f64),
    scale: f64,
) -> (f64, f64) {
    let margin = if cfg!(windows) { MARGIN } else { GAP } * scale;
    let right = work.right - w - margin;
    let bottom = work.bottom - h - margin;
    if !cfg!(windows) {
        let x = (icon_mid.0 - w / 2.0).clamp(work.left + margin, right.max(work.left));
        return (x, work.top + margin);
    }
    // The taskbar is the side where the work area stops short of the screen;
    // an auto-hidden one leaves no gap, so then go by the icon.
    let gaps = [
        work.top - screen.top,
        screen.bottom - work.bottom,
        work.left - screen.left,
        screen.right - work.right,
    ];
    let biggest = gaps.iter().copied().fold(0.0, f64::max);
    if biggest <= 0.0 {
        let near_top = icon_mid.1 < (screen.top + screen.bottom) / 2.0;
        return (right, if near_top { work.top + margin } else { bottom });
    }
    match gaps.iter().position(|g| *g == biggest) {
        Some(0) => (right, work.top + margin),
        Some(2) => (work.left + margin, bottom),
        _ => (right, bottom),
    }
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

/// Keep WebView2's drawing in place (decision #116): now, before the window
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

#[cfg(test)]
mod tests {
    use super::*;

    fn area(left: f64, top: f64, right: f64, bottom: f64) -> Area {
        Area {
            left,
            top,
            right,
            bottom,
        }
    }

    const SIZE: (f64, f64) = (340.0, 460.0);

    #[cfg(windows)]
    #[test]
    fn sits_above_a_bottom_taskbar_at_the_right_whatever_the_icon() {
        // Joshua's main screen: 2560 x 1440 at 100%, 48 px taskbar.
        let screen = area(0.0, 0.0, 2560.0, 1440.0);
        let work = area(0.0, 0.0, 2560.0, 1392.0);
        let expected = (2560.0 - 340.0 - 12.0, 1392.0 - 460.0 - 12.0);
        // On the taskbar, and in the hidden-icons overflow above it.
        assert_eq!(place(screen, work, (2300.0, 1416.0), SIZE, 1.0), expected);
        assert_eq!(place(screen, work, (2320.0, 1150.0), SIZE, 1.0), expected);
    }

    #[cfg(windows)]
    #[test]
    fn follows_the_taskbar_to_other_edges_and_scales() {
        let screen = area(-2560.0, 0.0, 0.0, 1440.0);
        // Top taskbar at 150%.
        let work = area(-2560.0, 72.0, 0.0, 1440.0);
        assert_eq!(
            place(screen, work, (-200.0, 30.0), (510.0, 690.0), 1.5),
            (-510.0 - 18.0, 72.0 + 18.0)
        );
        // Left taskbar.
        let work = area(-2488.0, 0.0, 0.0, 1440.0);
        assert_eq!(
            place(screen, work, (-2530.0, 1400.0), SIZE, 1.0),
            (-2488.0 + 12.0, 1440.0 - 460.0 - 12.0)
        );
        // Auto-hidden taskbar: by the icon, at the bottom.
        assert_eq!(
            place(screen, screen, (-100.0, 1420.0), SIZE, 1.0),
            (-340.0 - 12.0, 1440.0 - 460.0 - 12.0)
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn hangs_under_the_menu_bar_icon_inside_the_screen() {
        let screen = area(0.0, 0.0, 1512.0, 982.0);
        let work = area(0.0, 33.0, 1512.0, 982.0);
        assert_eq!(
            place(screen, work, (1200.0, 16.0), SIZE, 1.0),
            (1030.0, 39.0)
        );
        // Near the right edge it stops at the edge.
        assert_eq!(
            place(screen, work, (1500.0, 16.0), SIZE, 1.0),
            (1512.0 - 340.0 - 6.0, 39.0)
        );
    }
}
