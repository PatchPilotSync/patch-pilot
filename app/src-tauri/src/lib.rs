//! Patch Pilot desktop shell (#33). Commands here are the UI's only way into
//! the Rust crates; keep them thin and put logic in `crates/`.

mod background;
mod compare;
mod rig;
mod setup;
mod sync;
mod tray;

use serde::Serialize;

/// What `app_version` returns; mirrored by `AppVersion` in `src/api.ts`.
#[derive(Debug, Serialize)]
pub struct AppVersion {
    pub app: &'static str,
    pub manifest_schema: u32,
}

#[tauri::command]
fn app_version() -> AppVersion {
    AppVersion {
        app: env!("CARGO_PKG_VERSION"),
        manifest_schema: patchpilot_core::MANIFEST_SCHEMA_VERSION,
    }
}

pub fn run() {
    tauri::Builder::default()
        // Opening Patch Pilot again (Start menu, Finder) while it runs in the
        // tray shows the running one instead of starting a second.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tray::show_main(app);
        }))
        // Opens at login, hidden in the tray (decision #39). macOS uses a
        // LaunchAgent (the plug-in's default), which needs no permission prompt.
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .app_name("Patch Pilot")
                .arg(background::BACKGROUND_ARG)
                .build(),
        )
        // Self-update (decision #32): the UI checks once at launch and only
        // installs when the user says so.
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(rig::Scanning::default())
        .manage(tray::LastHidden::default())
        .setup(|app| {
            let app = app.handle();
            sync::setup(app);
            tray::setup(app)?;
            background::apply_login_default(app);
            if background::launched_at_login() {
                tray::set_dock_icon(app, false);
            } else {
                tray::show_main(app);
            }
            background::start(app);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_version,
            rig::overview,
            rig::scan_machine,
            rig::make_golden,
            compare::machines,
            compare::compare_machine,
            compare::plan_machine,
            compare::import_bundle,
            setup::setup_view,
            setup::setup_start,
            setup::setup_tick,
            setup::setup_finish,
            setup::setup_close,
            setup::setup_report_text,
            setup::setup_save_report,
            sync::sync_status,
            sync::link_start,
            sync::link_join,
            sync::unlink,
            tray::open_main_window,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Patch Pilot")
        .run(|_app, _event| {
            // macOS: clicking the Dock icon brings back a closed (hidden) window.
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = _event {
                tray::show_main(_app);
            }
        });
}

#[cfg(test)]
mod tests {
    #[test]
    fn app_version_reports_the_manifest_schema() {
        let v = super::app_version();
        assert_eq!(v.manifest_schema, patchpilot_core::MANIFEST_SCHEMA_VERSION);
        assert!(!v.app.is_empty());
    }
}
