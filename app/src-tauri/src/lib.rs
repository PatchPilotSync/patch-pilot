//! Patch Pilot desktop shell (#33). Commands here are the UI's only way into
//! the Rust crates; keep them thin and put logic in `crates/`.

mod compare;
mod rig;
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
        .manage(rig::Scanning::default())
        .manage(tray::LastHidden::default())
        .setup(|app| Ok(tray::setup(app.handle())?))
        .invoke_handler(tauri::generate_handler![
            app_version,
            rig::overview,
            rig::scan_machine,
            rig::make_golden,
            compare::machines,
            compare::compare_machine,
            compare::plan_machine,
            compare::import_bundle,
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
