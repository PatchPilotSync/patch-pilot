use std::path::{Path, PathBuf};

use patchpilot_local::restore::{
    ApplyError, ChangeSet, FileChange, apply_changes, go_back, go_back_preview, undo, undo_preview,
};
use patchpilot_store::{RestoreKind, Store};

struct Rig {
    _dir: tempfile::TempDir,
    files: PathBuf,
    blobs: PathBuf,
    store: Store,
}

fn rig() -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let files = dir.path().join("REAPER");
    std::fs::create_dir_all(files.join("TrackTemplates")).unwrap();
    Rig {
        files,
        blobs: dir.path().join("app-data").join("blobs"),
        store: Store::open_in_memory().unwrap(),
        _dir: dir,
    }
}

fn change(path: &Path, contents: Option<&str>) -> FileChange {
    FileChange {
        path: path.to_owned(),
        contents: contents.map(|c| c.as_bytes().to_vec()),
    }
}

fn set(label: &str, changes: Vec<FileChange>, now: i64) -> ChangeSet {
    ChangeSet {
        machine_id: "m1".into(),
        label: label.into(),
        daw: None,
        changes,
        now,
    }
}

fn read(p: &Path) -> Option<String> {
    std::fs::read_to_string(p).ok()
}

#[test]
fn apply_then_undo_restores_files_exactly() {
    let mut r = rig();
    let edited = r.files.join("TrackTemplates").join("Vocals.RTrackTemplate");
    let created = r.files.join("FXChains").join("New").join("Bus.RfxChain");
    let removed = r.files.join("TrackTemplates").join("Old.RTrackTemplate");
    std::fs::write(&edited, "old vocals").unwrap();
    std::fs::write(&removed, "old template").unwrap();

    let point = apply_changes(
        &mut r.store,
        &r.blobs,
        &set(
            "Brought over 3 Reaper files",
            vec![
                change(&edited, Some("new vocals")),
                change(&created, Some("bus chain")),
                change(&removed, None),
            ],
            10,
        ),
    )
    .unwrap();
    let id = point.id.unwrap();
    assert_eq!(point.changed.len(), 3);
    assert_eq!(read(&edited).as_deref(), Some("new vocals"));
    assert_eq!(read(&created).as_deref(), Some("bus chain"));
    assert_eq!(read(&removed), None);
    // No temp files or copies left beside the user's files.
    let names: Vec<_> = std::fs::read_dir(r.files.join("TrackTemplates"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(names, ["Vocals.RTrackTemplate"]);

    let rec = r.store.restore_point(id).unwrap().unwrap();
    assert_eq!(rec.kind, RestoreKind::Change);
    assert_eq!(rec.label, "Brought over 3 Reaper files");
    let preview = undo_preview(&r.blobs, &rec);
    assert_eq!(
        preview.iter().map(|f| f.restore).collect::<Vec<_>>(),
        [true, false, true]
    );
    assert!(
        preview
            .iter()
            .all(|f| !f.changed_since && !f.missing_backup)
    );

    let u = undo(&mut r.store, &r.blobs, id, 20).unwrap();
    assert_eq!(read(&edited).as_deref(), Some("old vocals"));
    assert_eq!(read(&created), None);
    assert_eq!(read(&removed).as_deref(), Some("old template"));

    let history = r.store.restore_points("m1").unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].kind, RestoreKind::Undo);
    assert_eq!(history[0].label, "Undid: Brought over 3 Reaper files");
    assert_eq!(history[0].undoes, Some(id));
    assert_eq!(history[1].undone_by, u.id);

    // The undo can be undone: back to the brought-over files.
    undo(&mut r.store, &r.blobs, u.id.unwrap(), 30).unwrap();
    assert_eq!(read(&edited).as_deref(), Some("new vocals"));
    assert_eq!(read(&created).as_deref(), Some("bus chain"));
    assert_eq!(read(&removed), None);
}

#[test]
fn unchanged_files_record_nothing() {
    let mut r = rig();
    let p = r.files.join("TrackTemplates").join("Same.RTrackTemplate");
    std::fs::write(&p, "same").unwrap();
    let point = apply_changes(
        &mut r.store,
        &r.blobs,
        &set("noop", vec![change(&p, Some("same"))], 1),
    )
    .unwrap();
    assert_eq!(point.id, None);
    assert!(r.store.restore_points("m1").unwrap().is_empty());
}

#[test]
fn preview_flags_files_edited_since() {
    let mut r = rig();
    let p = r.files.join("TrackTemplates").join("Drums.RTrackTemplate");
    std::fs::write(&p, "v1").unwrap();
    let id = apply_changes(
        &mut r.store,
        &r.blobs,
        &set("bring over", vec![change(&p, Some("v2"))], 1),
    )
    .unwrap()
    .id
    .unwrap();
    std::fs::write(&p, "v3, edited by hand").unwrap();
    let rec = r.store.restore_point(id).unwrap().unwrap();
    assert!(undo_preview(&r.blobs, &rec)[0].changed_since);

    // Undo still puts v1 back, and the hand edit is saved in the undo's own
    // restore point.
    let u = undo(&mut r.store, &r.blobs, id, 2).unwrap().id.unwrap();
    assert_eq!(read(&p).as_deref(), Some("v1"));
    undo(&mut r.store, &r.blobs, u, 3).unwrap();
    assert_eq!(read(&p).as_deref(), Some("v3, edited by hand"));
}

#[test]
fn failed_write_puts_back_earlier_files() {
    let mut r = rig();
    let ok = r.files.join("TrackTemplates").join("A.RTrackTemplate");
    std::fs::write(&ok, "before").unwrap();
    // A folder where the temp file goes makes the second write fail after
    // the first has happened.
    let bad = r.files.join("TrackTemplates").join("B.RTrackTemplate");
    std::fs::create_dir(
        r.files
            .join("TrackTemplates")
            .join(".B.RTrackTemplate.patchpilot-tmp"),
    )
    .unwrap();

    let err = apply_changes(
        &mut r.store,
        &r.blobs,
        &set(
            "half",
            vec![change(&ok, Some("after")), change(&bad, Some("x"))],
            1,
        ),
    )
    .unwrap_err();
    assert!(matches!(err, ApplyError::Io { .. }), "{err}");
    assert_eq!(read(&ok).as_deref(), Some("before"));
    assert!(r.store.restore_points("m1").unwrap().is_empty());
}

#[test]
fn rejects_relative_and_duplicate_paths() {
    let mut r = rig();
    let rel = apply_changes(
        &mut r.store,
        &r.blobs,
        &set("x", vec![change(Path::new("Rel.txt"), Some("x"))], 1),
    );
    assert!(matches!(rel, Err(ApplyError::BadPath(_))));
    let p = r.files.join("x.txt");
    let dup = apply_changes(
        &mut r.store,
        &r.blobs,
        &set("x", vec![change(&p, Some("a")), change(&p, Some("b"))], 1),
    );
    assert!(matches!(dup, Err(ApplyError::Duplicate(_))));
    assert_eq!(read(&p), None);
}

#[test]
fn missing_backup_is_reported_not_guessed() {
    let mut r = rig();
    let p = r.files.join("TrackTemplates").join("Gone.RTrackTemplate");
    std::fs::write(&p, "v1").unwrap();
    let id = apply_changes(
        &mut r.store,
        &r.blobs,
        &set("x", vec![change(&p, Some("v2"))], 1),
    )
    .unwrap()
    .id
    .unwrap();
    std::fs::remove_dir_all(&r.blobs).unwrap();
    let rec = r.store.restore_point(id).unwrap().unwrap();
    assert!(undo_preview(&r.blobs, &rec)[0].missing_backup);
    assert!(matches!(
        undo(&mut r.store, &r.blobs, id, 2),
        Err(ApplyError::MissingBackup(_))
    ));
    assert_eq!(read(&p).as_deref(), Some("v2"));
}

#[test]
fn go_back_restores_files_as_they_were_at_a_scan() {
    let mut r = rig();
    let m = patchpilot_core::Manifest::new(
        patchpilot_core::MachineInfo {
            id: "m1".into(),
            name: "Studio".into(),
            os: patchpilot_core::Os::Macos,
            arch: patchpilot_core::Arch::Aarch64,
            os_version: None,
            path_map: patchpilot_core::PathMap::new(patchpilot_core::Os::Macos),
            disks: Vec::new(),
        },
        1,
    );
    let early = r
        .store
        .save_manifest(&m, patchpilot_store::SnapshotReason::FirstScan, 1)
        .unwrap()
        .id;

    let a = r.files.join("TrackTemplates").join("A.RTrackTemplate");
    let b = r.files.join("TrackTemplates").join("B.RTrackTemplate");
    std::fs::write(&a, "a1").unwrap();
    apply_changes(
        &mut r.store,
        &r.blobs,
        &set(
            "one",
            vec![change(&a, Some("a2")), change(&b, Some("b"))],
            2,
        ),
    )
    .unwrap();
    apply_changes(
        &mut r.store,
        &r.blobs,
        &set("two", vec![change(&a, Some("a3"))], 3),
    )
    .unwrap();

    let preview = go_back_preview(&r.store, &r.blobs, "m1", early).unwrap();
    assert_eq!(preview.len(), 2);
    assert!(preview.iter().all(|f| !f.changed_since));

    // A later scan: going back to it changes nothing.
    let mut later = m.clone();
    later.machine.name = "Studio 2".into();
    later.generated_at = 4;
    let late = r
        .store
        .save_manifest(&later, patchpilot_store::SnapshotReason::Scan, 4)
        .unwrap()
        .id;
    assert!(
        go_back_preview(&r.store, &r.blobs, "m1", late)
            .unwrap()
            .is_empty()
    );

    let back = go_back(&mut r.store, &r.blobs, "m1", early, "Went back", 5).unwrap();
    assert_eq!(read(&a).as_deref(), Some("a1"));
    assert_eq!(read(&b), None);
    // And it can be undone.
    undo(&mut r.store, &r.blobs, back.id.unwrap(), 6).unwrap();
    assert_eq!(read(&a).as_deref(), Some("a3"));
    assert_eq!(read(&b).as_deref(), Some("b"));
}
