use std::path::PathBuf;

use patchpilot_core::Manifest;
use patchpilot_store::{LATEST_VERSION, SnapshotReason, Store, StoreError};

fn fixture_manifest() -> Manifest {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/core/manifest-v1/studio-pc.json");
    Manifest::from_json(&std::fs::read_to_string(path).unwrap()).unwrap()
}

const TABLES: &[&str] = &[
    "machines",
    "products",
    "items",
    "snapshots",
    "snapshot_items",
    "profiles",
    "profile_rules",
    "path_maps",
    "vault_entries",
    "dependencies",
    "events",
    "decisions",
];

#[test]
fn migrates_an_empty_database() {
    let store = Store::open_in_memory().unwrap();
    assert_eq!(store.schema_version().unwrap(), LATEST_VERSION);
    for table in TABLES {
        let n: i64 = store
            .connection()
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1, "table {table}");
    }
}

#[test]
fn reopening_is_idempotent_and_on_disk() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested").join("patchpilot.db");
    {
        let mut store = Store::open(&path).unwrap();
        store
            .save_manifest(&fixture_manifest(), SnapshotReason::FirstScan, 1)
            .unwrap();
    }
    let store = Store::open(&path).unwrap();
    assert_eq!(store.schema_version().unwrap(), LATEST_VERSION);
    assert!(
        store
            .latest_manifest("6f1c2a9e-0d4b-4c57-9a43-1f0e8b7d2c11")
            .unwrap()
            .is_some()
    );
}

#[test]
fn refuses_a_newer_database() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("patchpilot.db");
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.pragma_update(None, "user_version", LATEST_VERSION + 1)
            .unwrap();
    }
    assert!(matches!(Store::open(&path), Err(StoreError::TooNew { .. })));
}

#[test]
fn manifest_round_trips_through_the_db() {
    let manifest = fixture_manifest();
    let mut store = Store::open_in_memory().unwrap();
    let snap = store
        .save_manifest(&manifest, SnapshotReason::FirstScan, 1_800_000_000)
        .unwrap();
    assert!(!snap.unchanged);
    assert!(snap.manifest_hash.starts_with("blake3:"));

    let loaded = store
        .latest_manifest(&manifest.machine.id)
        .unwrap()
        .unwrap();
    assert_eq!(loaded, manifest);
}

#[test]
fn identical_scan_reuses_the_snapshot_and_changes_share_rows() {
    let manifest = fixture_manifest();
    let mut store = Store::open_in_memory().unwrap();
    let first = store
        .save_manifest(&manifest, SnapshotReason::FirstScan, 1)
        .unwrap();
    let again = store
        .save_manifest(&manifest, SnapshotReason::Scan, 2)
        .unwrap();
    assert!(again.unchanged);
    assert_eq!(again.id, first.id);

    let mut changed = manifest.clone();
    changed.items[0].version = Some("2.2.0".into());
    let second = store
        .save_manifest(&changed, SnapshotReason::Scan, 3)
        .unwrap();
    assert_eq!(second.parent_id, Some(first.id));

    let items: i64 = store
        .connection()
        .query_row("SELECT count(*) FROM items", [], |r| r.get(0))
        .unwrap();
    // 4 original rows + 1 for the changed item; unchanged items are shared.
    assert_eq!(items, 5);

    // Both snapshots stay readable as they were.
    assert_eq!(store.snapshot_manifest(first.id).unwrap(), manifest);
    assert_eq!(
        store
            .latest_manifest(&manifest.machine.id)
            .unwrap()
            .unwrap(),
        changed
    );
}

#[test]
fn unknown_machine() {
    let store = Store::open_in_memory().unwrap();
    assert!(store.latest_manifest("nope").unwrap().is_none());
    assert!(matches!(
        store.machine("nope"),
        Err(StoreError::UnknownMachine(_))
    ));
}
