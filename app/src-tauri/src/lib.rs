//! Patch Pilot desktop shell (#33). Commands here are the UI's only way into
//! the Rust crates; keep them thin and put logic in `crates/`.

mod alerts;
mod autosync;
mod background;
mod backup;
mod bring;
mod catalog;
mod compare;
mod duplicates;
mod github;
mod history;
mod mac_keys;
mod reaper;
mod report;
mod rig;
mod setup;
mod sync;
mod transfer;
mod tray;
mod vault;

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

/// Show a file the app listed (a skipped backup file, a duplicate preset)
/// selected in Finder or Explorer. Only shows it: nothing is opened or run.
#[tauri::command]
async fn reveal_path(path: String) -> Result<(), String> {
    let path = std::path::PathBuf::from(path);
    if !path.is_absolute() || !path.exists() {
        return Err("That file isn't there any more.".into());
    }
    patchpilot_local::open::reveal(&path).map_err(|e| e.to_string())
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
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(rig::Scanning::default())
        .manage(tray::LastHidden::default())
        .setup(|app| {
            let app = app.handle();
            mac_keys::setup(app);
            sync::setup(app);
            alerts::setup(app);
            catalog::setup(app);
            vault::setup(app);
            transfer::setup(app);
            backup::setup(app);
            autosync::setup(app);
            github::setup(app);
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
            reveal_path,
            rig::overview,
            rig::scan_machine,
            rig::make_golden,
            compare::machines,
            compare::compare_machine,
            compare::plan_machine,
            compare::plan_progress,
            compare::plan_tick,
            compare::import_bundle,
            setup::setup_view,
            setup::setup_start,
            setup::setup_tick,
            setup::setup_finish,
            setup::setup_close,
            setup::setup_report_text,
            setup::setup_save_report,
            compare::add_to_golden,
            compare::update_golden_from,
            compare::ignore_suggestions,
            compare::unignore_suggestions,
            compare::ignore_items,
            compare::unignore,
            compare::ignore_list,
            history::history,
            history::undo_preview,
            history::undo_change,
            history::go_back_plan,
            history::go_back,
            reaper::reaper_extras,
            reaper::restore_reapack,
            reaper::fix_reaper_folders,
            bring::files_to_bring,
            bring::bring_files,
            transfer::transfer_view,
            transfer::set_transfer_folder,
            transfer::send_big_files,
            backup::backup_view,
            backup::set_backup_folder,
            backup::set_backup_auto,
            backup::back_up_now,
            backup::backup_restore_list,
            backup::restore_backup,
            autosync::sync_view,
            autosync::set_keep_in_sync,
            autosync::settle_conflict,
            autosync::sync_look_now,
            autosync::sync_prefs_view,
            mac_keys::mac_keys_view,
            mac_keys::set_mac_keys,
            autosync::set_sync_kind,
            github::github_view,
            github::github_sign_in,
            github::github_cancel_sign_in,
            github::github_use_token,
            github::github_use_repo,
            github::github_sign_out,
            github::github_open,
            duplicates::find_duplicates,
            duplicates::clean_up_duplicates,
            catalog::updates,
            report::rig_report_preview,
            report::save_rig_report,
            sync::sync_status,
            sync::link_start,
            sync::link_join,
            sync::unlink,
            tray::open_main_window,
            vault::vault_status,
            vault::vault_set_root,
            vault::vault_add,
            vault::vault_inspect,
            vault::vault_skip,
            vault::vault_remove,
            vault::vault_matches,
            vault::vault_run,
            vault::vault_reveal,
            vault::vendor_apps,
            vault::open_vendor_app,
            vault::open_web_page,
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
