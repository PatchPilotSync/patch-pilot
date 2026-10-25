//! Live sync on this machine (decision #38): the rig key file in app-data, and
//! turning the local store into what goes to the relay and back. No network;
//! the app moves the bytes.

use std::path::{Path, PathBuf};

use patchpilot_store::{ImportReport, Store};
use patchpilot_sync::{RigKey, machine_object};

use crate::now;

const RIG_KEY_FILE: &str = "rig-key";

/// The rig key this machine is linked with, if any.
pub fn load_rig(app_dir: &Path) -> Result<Option<RigKey>, String> {
    let path = rig_key_path(app_dir);
    match std::fs::read_to_string(&path) {
        Ok(s) => RigKey::from_stored(&s)
            .map(Some)
            .map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// Keep the rig key in app-data, written to a temp name and renamed.
pub fn save_rig(app_dir: &Path, rig: &RigKey) -> Result<(), String> {
    let path = rig_key_path(app_dir);
    let tmp = app_dir.join(format!("{RIG_KEY_FILE}.tmp"));
    std::fs::create_dir_all(app_dir)
        .and_then(|()| std::fs::write(&tmp, rig.to_stored() + "\n"))
        .and_then(|()| std::fs::rename(&tmp, &path))
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// Unlink this machine: forget the rig key. Scans already received stay.
pub fn forget_rig(app_dir: &Path) -> Result<(), String> {
    let path = rig_key_path(app_dir);
    match std::fs::remove_file(&path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            Err(format!("{}: {e}", path.display()))
        }
        _ => Ok(()),
    }
}

fn rig_key_path(app_dir: &Path) -> PathBuf {
    app_dir.join(RIG_KEY_FILE)
}

/// This machine's object for the relay: its latest scan, every Primary Rig version
/// and the user's profiles, sealed. `None` before the first saved scan.
///
/// Paths stay as they are: only this user's own machines can read it.
pub fn outgoing(
    store: &Store,
    this: &str,
    rig: &RigKey,
) -> Result<Option<(String, Vec<u8>)>, String> {
    if store.latest_manifest(this).map_err(err)?.is_none() {
        return Ok(None);
    }
    let name = store.machine(this).ok().map(|m| m.name);
    let bundle = store
        .export_bundle(&[this.to_owned()], name.as_deref(), now())
        .map_err(err)?;
    let object = machine_object(this).map_err(|e| e.to_string())?;
    let sealed = rig
        .seal_bundle(&object, &bundle)
        .map_err(|e| e.to_string())?;
    Ok(Some((object, sealed)))
}

/// Whether an object on the relay is another machine's, so worth fetching.
pub fn is_other_machine(object: &str, this: &str) -> bool {
    object.starts_with("machine/") && Some(object) != machine_object(this).ok().as_deref()
}

/// Open another machine's object and add what's new, with the same add-only
/// rules as an export file.
pub fn incoming(
    store: &mut Store,
    this: &str,
    rig: &RigKey,
    object: &str,
    sealed: &[u8],
) -> Result<ImportReport, String> {
    let bundle = rig.open_bundle(object, sealed).map_err(|e| e.to_string())?;
    store.import_bundle(&bundle, Some(this), now()).map_err(err)
}

/// True when an import changed anything the UI shows.
pub fn changed(report: &ImportReport) -> bool {
    !report.machines_added.is_empty()
        || !report.profiles_added.is_empty()
        || report.ignored_added > 0
        || matches!(report.golden, patchpilot_store::GoldenImport::Imported(_))
}

fn err(e: patchpilot_store::StoreError) -> String {
    e.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use patchpilot_core::Manifest;

    fn load(rel: &str) -> Manifest {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures")
            .join(rel);
        Manifest::from_json(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn rig_key_file_round_trips_and_is_forgotten() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("PatchPilot");
        assert_eq!(load_rig(&app).unwrap(), None);
        let rig = RigKey::generate();
        save_rig(&app, &rig).unwrap();
        assert_eq!(load_rig(&app).unwrap(), Some(rig));
        assert!(!app.join("rig-key.tmp").exists());
        forget_rig(&app).unwrap();
        forget_rig(&app).unwrap();
        assert_eq!(load_rig(&app).unwrap(), None);
    }

    #[test]
    fn a_pc_and_a_mac_see_each_other_through_sealed_objects() {
        let studio = load("parity/score/golden.json");
        let laptop = load("parity/score/laptop.json");
        let rig = RigKey::generate();

        let mut pc = Store::open_in_memory().unwrap();
        crate::save_scan(&mut pc, &studio).unwrap();
        pc.save_golden(&studio, Some(&studio.machine.id), None, 1)
            .unwrap();
        let mut mac = Store::open_in_memory().unwrap();
        assert_eq!(outgoing(&mac, &laptop.machine.id, &rig).unwrap(), None);
        crate::save_scan(&mut mac, &laptop).unwrap();

        // PC uploads; the Mac gets the PC's scan and the Primary Rig.
        let (object, sealed) = outgoing(&pc, &studio.machine.id, &rig).unwrap().unwrap();
        assert!(is_other_machine(&object, &laptop.machine.id));
        assert!(!is_other_machine(&object, &studio.machine.id));
        let report = incoming(&mut mac, &laptop.machine.id, &rig, &object, &sealed).unwrap();
        assert!(changed(&report));
        assert!(mac.golden(None).unwrap().is_some());
        // Again: nothing new.
        let report = incoming(&mut mac, &laptop.machine.id, &rig, &object, &sealed).unwrap();
        assert!(!changed(&report));

        // Mac uploads; the PC gets the Mac's scan.
        let (object, sealed) = outgoing(&mac, &laptop.machine.id, &rig).unwrap().unwrap();
        let report = incoming(&mut pc, &studio.machine.id, &rig, &object, &sealed).unwrap();
        assert_eq!(report.machines_added, vec![laptop.machine.name.clone()]);
        assert_eq!(pc.machines().unwrap().len(), 2);

        // Another rig's key can't read it.
        assert!(
            incoming(
                &mut pc,
                &studio.machine.id,
                &RigKey::generate(),
                &object,
                &sealed
            )
            .is_err()
        );
    }
}
