//! Patch Pilot desktop shell (#33). Commands here are the UI's only way into
//! the Rust crates; keep them thin and put logic in `crates/`.

mod compare;
mod rig;

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
        .invoke_handler(tauri::generate_handler![
            app_version,
            rig::overview,
            rig::scan_machine,
            rig::make_golden,
            compare::machines,
            compare::compare_machine,
            compare::plan_machine,
            compare::import_bundle,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Patch Pilot");
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
