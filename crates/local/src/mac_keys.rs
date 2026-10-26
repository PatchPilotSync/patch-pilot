//! Reaper shortcuts on a Mac (decision #81): unless turned off, this Mac's
//! `reaper-kb.ini` is kept with the shortcuts macOS keeps for itself moved to
//! Control ([`patchpilot_parity::reaper_keys`]). Turning it on or off moves
//! this machine's file once, while Reaper is closed, in a restore point;
//! until then it's shared and written in the form it's in. After that, what
//! comes from other machines is moved on the way in and moved back on the way
//! out, so every machine shares the same shortcuts in the Windows form.

use std::path::Path;

use patchpilot_core::{Manifest, Os};
use patchpilot_parity::reaper::REAPER_RESOURCES;
use patchpilot_parity::reaper_keys::{MacShortcuts, for_mac, mac_shortcuts, set_mac_form};
use patchpilot_store::Store;
use serde::Serialize;

use crate::restore::{Daw, daw_running};
use crate::{bring, sync_prefs};

const KEY_MAP: &str = "reaper-kb.ini";
/// The restore point labels; undoing one turns the setting the other way.
pub const MOVED: &str = "Moved Reaper shortcuts macOS keeps for itself to Control";
pub const PUT_BACK: &str = "Put Reaper shortcuts back on the keys they came with";

/// Whether this machine's key map should be kept moved.
fn wanted(prefs: &sync_prefs::SyncPrefs, os: Os) -> bool {
    os == Os::Macos && !prefs.mac_shortcuts_off
}

/// Tell the scanner and Bring over which form this machine's key map is in.
/// Call at start-up, before the first scan.
pub fn load(app_dir: &Path) {
    set_mac_form(sync_prefs::load(app_dir).mac_form);
}

/// Bring this machine's key map in line with the setting: moved (or moved
/// back) once Reaper is closed. `Ok(true)` while it waits for Reaper to close.
pub fn settle(
    store: &mut Store,
    blobs: &Path,
    this: &str,
    app_dir: &Path,
    os: Os,
    now: i64,
) -> Result<bool, String> {
    let mut prefs = sync_prefs::load(app_dir);
    set_mac_form(prefs.mac_form);
    let want = wanted(&prefs, os);
    if want == prefs.mac_form {
        return Ok(false);
    }
    let machine = store.latest_manifest(this).map_err(|e| e.to_string())?;
    let text = bring::root_dir(machine.as_ref(), REAPER_RESOURCES)
        .and_then(|dir| Some((std::fs::read_to_string(dir.join(KEY_MAP)).ok()?, dir)));
    if let Some((text, dir)) = text {
        let moved = for_mac(&text);
        if moved != text {
            if daw_running(Daw::Reaper) {
                return Ok(true);
            }
            let label = if want { MOVED } else { PUT_BACK };
            bring::write_files(
                store,
                blobs,
                this,
                label,
                REAPER_RESOURCES,
                &dir,
                &[(KEY_MAP.to_owned(), moved.into_bytes())],
                now,
            )?;
        }
    }
    prefs.mac_form = want;
    sync_prefs::save(app_dir, &prefs)?;
    set_mac_form(want);
    Ok(false)
}

/// After History undid the restore point labelled `undone`: if it was one of
/// ours (or an undo of one), the key map is in the other form now, so the
/// setting follows it rather than moving it straight back.
pub fn after_undo(app_dir: &Path, undone: &str) -> Result<(), String> {
    let Some(moved_now) = moved_after_undo(undone) else {
        return Ok(());
    };
    let mut prefs = sync_prefs::load(app_dir);
    prefs.mac_form = moved_now;
    prefs.mac_shortcuts_off = !moved_now;
    sync_prefs::save(app_dir, &prefs)?;
    set_mac_form(moved_now);
    Ok(())
}

/// Whether the key map is kept moved after undoing the restore point
/// labelled `undone`, if that was one of ours or an undo of one.
fn moved_after_undo(undone: &str) -> Option<bool> {
    let mut label = undone;
    let mut undos = 0;
    while let Some(rest) = label.strip_prefix("Undid: ") {
        label = rest;
        undos += 1;
    }
    match label {
        MOVED => Some(undos % 2 == 1),
        PUT_BACK => Some(undos % 2 == 0),
        _ => None,
    }
}

/// The setting and what it does to this machine's shortcuts. Mirrored in
/// `app/src/api.ts`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct MacKeysView {
    pub on: bool,
    /// This machine is a Mac, so the setting applies here.
    pub here: bool,
    /// Turned on or off, and waiting for Reaper to close to move them.
    pub waiting: bool,
    /// What it moves (or would, on a Mac) in this machine's key map.
    pub shortcuts: MacShortcuts,
}

pub fn view(app_dir: &Path, machine: Option<&Manifest>, os: Os) -> MacKeysView {
    let prefs = sync_prefs::load(app_dir);
    let shortcuts = bring::root_dir(machine, REAPER_RESOURCES)
        .and_then(|dir| bring::shared_copy(&dir, REAPER_RESOURCES, KEY_MAP))
        .map(|bytes| mac_shortcuts(&String::from_utf8_lossy(&bytes)))
        .unwrap_or_default();
    MacKeysView {
        on: !prefs.mac_shortcuts_off,
        here: os == Os::Macos,
        waiting: wanted(&prefs, os) != prefs.mac_form,
        shortcuts,
    }
}

/// Turn the setting on or off for this machine; [`settle`] moves the file.
pub fn set(app_dir: &Path, on: bool) -> Result<(), String> {
    let mut prefs = sync_prefs::load(app_dir);
    prefs.mac_shortcuts_off = !on;
    sync_prefs::save(app_dir, &prefs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undoing_a_move_turns_the_setting_off_and_redoing_it_on() {
        assert_eq!(moved_after_undo(MOVED), Some(false));
        assert_eq!(moved_after_undo(&format!("Undid: {MOVED}")), Some(true));
        assert_eq!(moved_after_undo(PUT_BACK), Some(true));
        assert_eq!(
            moved_after_undo(&format!("Undid: Undid: {PUT_BACK}")),
            Some(true)
        );
        assert_eq!(moved_after_undo("Kept in sync: 1 Reaper file"), None);
    }

    #[test]
    fn only_macs_keep_them_moved() {
        let mut prefs = sync_prefs::SyncPrefs::default();
        assert!(wanted(&prefs, Os::Macos));
        assert!(!wanted(&prefs, Os::Windows));
        prefs.mac_shortcuts_off = true;
        assert!(!wanted(&prefs, Os::Macos));
    }
}
