use std::path::PathBuf;

use patchpilot_core::{
    BUNDLE_VERSION, Bundle, BundleMachine, Manifest, ManifestError, Matcher, PluginFormat,
    PortablePath, ProfileRule, RuleAction, TargetProfile,
};
use patchpilot_store::{
    GoldenImport, ImportReport, LATEST_VERSION, SnapshotReason, Store, StoreError,
};

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
    "golden_versions",
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

#[test]
fn rescan_with_a_new_time_is_unchanged() {
    let manifest = fixture_manifest();
    let mut store = Store::open_in_memory().unwrap();
    let first = store
        .save_manifest(&manifest, SnapshotReason::FirstScan, 1)
        .unwrap();
    let mut rescan = manifest.clone();
    rescan.generated_at += 3600;
    let again = store
        .save_manifest(&rescan, SnapshotReason::Scan, 2)
        .unwrap();
    assert!(again.unchanged);
    assert_eq!(again.id, first.id);
    assert_eq!(again.manifest_hash, first.manifest_hash);
}

#[test]
fn history_lists_snapshots_newest_first_with_changes() {
    let manifest = fixture_manifest();
    let id = manifest.machine.id.clone();
    let mut store = Store::open_in_memory().unwrap();
    assert!(store.snapshots(&id).unwrap().is_empty());
    store
        .save_manifest(&manifest, SnapshotReason::FirstScan, 10)
        .unwrap();
    let mut changed = manifest.clone();
    changed.items[0].version = Some("2.2.0".into());
    changed.items.pop();
    store
        .save_manifest(&changed, SnapshotReason::Scan, 20)
        .unwrap();

    let history = store.snapshots(&id).unwrap();
    assert_eq!(history.len(), 2);
    let (newest, oldest) = (&history[0], &history[1]);
    assert_eq!(newest.reason, SnapshotReason::Scan);
    assert_eq!(newest.created_at, 20);
    assert_eq!(newest.parent_id, Some(oldest.id));
    assert_eq!(newest.items, manifest.items.len() as u64 - 1);
    assert_eq!((newest.added, newest.removed), (1, 2));
    assert_eq!(oldest.reason, SnapshotReason::FirstScan);
    assert_eq!(oldest.parent_id, None);
    assert_eq!(oldest.added, manifest.items.len() as u64);
    assert_eq!(oldest.removed, 0);
}

#[test]
fn golden_versions_are_kept_and_chained() {
    let studio = fixture_manifest();
    let mut store = Store::open_in_memory().unwrap();
    assert!(store.golden(None).unwrap().is_none());

    let v1 = store
        .save_golden(&studio, Some(&studio.machine.id), Some("first rig"), 100)
        .unwrap();
    assert_eq!((v1.version, v1.parent), (1, None));

    let mut updated = studio.clone();
    updated.items[0].version = Some("9.9.9".into());
    let v2 = store.save_golden(&updated, None, None, 200).unwrap();
    assert_eq!((v2.version, v2.parent), (2, Some(1)));

    // v1 stays readable exactly as it was, under its own name.
    let g1 = store.golden(Some(1)).unwrap().unwrap();
    assert_eq!(g1.info, v1);
    assert_eq!(g1.manifest.machine.id, "golden@1");
    assert_eq!(g1.manifest.machine.name, "Golden Rig v1");
    assert_eq!(g1.manifest.items, studio.items);
    assert_eq!(g1.manifest.machine.path_map, studio.machine.path_map);

    let latest = store.golden(None).unwrap().unwrap();
    assert_eq!(latest.info.version, 2);
    assert_eq!(latest.manifest.items, updated.items);
    assert!(store.golden(Some(3)).unwrap().is_none());

    // Saving the same content again is refused; history doesn't grow.
    let mut same = updated.clone();
    same.generated_at += 60;
    assert!(matches!(
        store.save_golden(&same, None, None, 300),
        Err(StoreError::GoldenUnchanged(2))
    ));
    let history: Vec<_> = store
        .golden_history()
        .unwrap()
        .iter()
        .map(|v| v.version)
        .collect();
    assert_eq!(history, [2, 1]);

    // Golden versions don't show up as the scanned machine's history.
    assert_eq!(store.snapshots(&studio.machine.id).unwrap().len(), 0);
}

#[test]
fn migrates_a_v1_database_to_latest() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("patchpilot.db");
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(patchpilot_store::MIGRATIONS[0].1)
            .unwrap();
        conn.pragma_update(None, "user_version", 1).unwrap();
    }
    let mut store = Store::open(&path).unwrap();
    assert_eq!(store.schema_version().unwrap(), LATEST_VERSION);
    assert!(store.target_profile("travel").unwrap().is_some());
    store
        .save_golden(&fixture_manifest(), None, None, 1)
        .unwrap();
    assert_eq!(store.golden(None).unwrap().unwrap().info.version, 1);
}

#[test]
fn built_in_target_profiles() {
    let store = Store::open_in_memory().unwrap();
    let all = store.target_profiles().unwrap();
    let ids: Vec<_> = all.iter().map(|p| p.profile.id.as_str()).collect();
    assert_eq!(ids, ["studio-clone", "travel"]);
    assert!(all.iter().all(|p| p.builtin && p.machines.is_empty()));
    assert!(
        all[0].profile.rules.is_empty(),
        "studio clone excludes nothing"
    );

    // Travel matches the JSON profile alpha testers already use.
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/parity/score/travel-profile.json");
    let file: TargetProfile =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(store.target_profile("travel").unwrap().unwrap(), file);
    assert!(store.target_profile("golden").unwrap().is_none());
}

#[test]
fn import_assign_and_protect_profiles() {
    let manifest = fixture_manifest();
    let id = manifest.machine.id.clone();
    let mut store = Store::open_in_memory().unwrap();
    store
        .save_manifest(&manifest, SnapshotReason::FirstScan, 1)
        .unwrap();
    assert_eq!(store.machine_profile(&id).unwrap(), None);

    let mut custom = TargetProfile {
        id: "laptop".into(),
        name: "Laptop".into(),
        rules: vec![ProfileRule {
            action: RuleAction::Exclude,
            matcher: Matcher::Format(PluginFormat::Aax),
        }],
    };
    store.save_target_profile(&custom).unwrap();
    custom.name = "Laptop (no AAX, no VST2)".into();
    custom.rules.push(ProfileRule {
        action: RuleAction::Exclude,
        matcher: Matcher::Format(PluginFormat::Vst2),
    });
    store.save_target_profile(&custom).unwrap();
    assert_eq!(store.target_profile("laptop").unwrap().unwrap(), custom);

    store.set_machine_profile(&id, Some("laptop")).unwrap();
    assert_eq!(store.machine_profile(&id).unwrap(), Some(custom.clone()));
    let laptop = store
        .target_profiles()
        .unwrap()
        .into_iter()
        .find(|p| p.profile.id == "laptop")
        .unwrap();
    assert_eq!((laptop.builtin, laptop.machines), (false, vec![id.clone()]));

    // A rescan keeps the assignment.
    let mut rescan = manifest.clone();
    rescan.items.pop();
    store
        .save_manifest(&rescan, SnapshotReason::Scan, 2)
        .unwrap();
    assert_eq!(store.machine_profile(&id).unwrap(), Some(custom));

    for bad in ["travel", "golden", ""] {
        let p = TargetProfile {
            id: bad.into(),
            ..TargetProfile::default()
        };
        assert!(
            matches!(
                store.save_target_profile(&p),
                Err(StoreError::BuiltinProfile(_))
            ),
            "{bad:?}"
        );
    }
    assert!(matches!(
        store.set_machine_profile(&id, Some("nope")),
        Err(StoreError::UnknownProfile(_))
    ));
    assert!(matches!(
        store.set_machine_profile("nope", Some("travel")),
        Err(StoreError::UnknownMachine(_))
    ));
    store.set_machine_profile(&id, None).unwrap();
    assert_eq!(store.machine_profile(&id).unwrap(), None);
}

#[test]
fn machines_lists_scanned_machines_only() {
    let manifest = fixture_manifest();
    let mut store = Store::open_in_memory().unwrap();
    store
        .save_manifest(&manifest, SnapshotReason::FirstScan, 1)
        .unwrap();
    store.save_golden(&manifest, None, None, 2).unwrap();
    let ids: Vec<_> = store
        .machines()
        .unwrap()
        .into_iter()
        .map(|m| m.id)
        .collect();
    assert_eq!(ids, [manifest.machine.id]);
}

#[test]
fn requires_round_trips() {
    let mut manifest = fixture_manifest();
    manifest.items[0].requires = vec!["acme/a".into(), "acme/b".into()];
    let mut store = Store::open_in_memory().unwrap();
    store
        .save_manifest(&manifest, SnapshotReason::FirstScan, 1)
        .unwrap();
    let loaded = store
        .latest_manifest(&manifest.machine.id)
        .unwrap()
        .unwrap();
    assert_eq!(loaded, manifest);
}

fn mac_manifest() -> Manifest {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/parity/diff/laptop-mac.json");
    Manifest::from_json(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn laptop_profile() -> TargetProfile {
    TargetProfile {
        id: "laptop".into(),
        name: "Laptop".into(),
        rules: vec![ProfileRule {
            action: RuleAction::Exclude,
            matcher: Matcher::Format(PluginFormat::Aax),
        }],
    }
}

#[test]
fn bundle_moves_machines_golden_and_profiles_between_stores() {
    let studio = fixture_manifest();
    let mac = mac_manifest();

    // Windows studio PC: its own scan, a Golden v1 and a custom profile
    // assigned to the Mac it knows about.
    let mut win = Store::open_in_memory().unwrap();
    win.save_manifest(&studio, SnapshotReason::FirstScan, 1)
        .unwrap();
    win.save_manifest(&mac, SnapshotReason::Imported, 1)
        .unwrap();
    win.save_golden(&studio, Some(&studio.machine.id), Some("first rig"), 2)
        .unwrap();
    win.save_target_profile(&laptop_profile()).unwrap();
    win.set_machine_profile(&mac.machine.id, Some("laptop"))
        .unwrap();

    let ids = [studio.machine.id.clone(), mac.machine.id.clone()];
    let bundle = win
        .export_bundle(&ids, Some(&studio.machine.name), 10)
        .unwrap();
    assert_eq!(bundle.golden.len(), 1);
    assert_eq!(
        bundle.profiles,
        [laptop_profile()],
        "built-ins not exported"
    );
    let bundle = Bundle::from_json(&bundle.to_json_pretty()).unwrap();

    // The Mac imports it: the studio PC and Golden arrive, its own scan is
    // skipped, the profile and its own assignment come along.
    let mut mac_store = Store::open_in_memory().unwrap();
    mac_store
        .save_manifest(&mac, SnapshotReason::FirstScan, 5)
        .unwrap();
    let report = mac_store
        .import_bundle(&bundle, Some(&mac.machine.id), 20)
        .unwrap();
    assert_eq!(
        report,
        ImportReport {
            machines_added: vec![studio.machine.name.clone()],
            machines_unchanged: vec![],
            skipped_own_machine: true,
            golden: GoldenImport::Imported(vec![1]),
            profiles_added: vec!["laptop".into()],
            profiles_conflicting: vec![],
        }
    );
    assert_eq!(
        mac_store
            .latest_manifest(&studio.machine.id)
            .unwrap()
            .unwrap()
            .items,
        studio.items
    );
    let history = mac_store.snapshots(&studio.machine.id).unwrap();
    assert_eq!(history[0].reason, SnapshotReason::Imported);
    let g = mac_store.golden(None).unwrap().unwrap();
    assert_eq!(g.manifest.items, studio.items);
    assert_eq!(
        g.info.source_machine_id.as_deref(),
        Some(&*studio.machine.id)
    );
    // The Mac's own assignment is the Mac's business: not imported.
    assert_eq!(mac_store.machine_profile(&mac.machine.id).unwrap(), None);

    // Importing again changes nothing.
    let again = mac_store
        .import_bundle(&bundle, Some(&mac.machine.id), 30)
        .unwrap();
    assert_eq!(
        again.machines_unchanged,
        std::slice::from_ref(&studio.machine.name)
    );
    assert!(again.machines_added.is_empty() && again.profiles_added.is_empty());
    assert_eq!(again.golden, GoldenImport::UpToDate);
    assert_eq!(mac_store.snapshots(&studio.machine.id).unwrap().len(), 1);

    // A third machine with no scan of its own gets the Mac's assignment too.
    let mut third = Store::open_in_memory().unwrap();
    third.import_bundle(&bundle, None, 40).unwrap();
    assert_eq!(
        third.machine_profile(&mac.machine.id).unwrap(),
        Some(laptop_profile())
    );
}

#[test]
fn golden_import_only_fast_forwards() {
    let studio = fixture_manifest();
    let mut v2 = studio.clone();
    v2.items[0].version = Some("9.9.9".into());
    let mut v3 = v2.clone();
    v3.items.pop();

    let mut source = Store::open_in_memory().unwrap();
    for (m, t) in [(&studio, 1), (&v2, 2), (&v3, 3)] {
        source.save_golden(m, None, None, t).unwrap();
    }
    let mut bundle = source.export_bundle(&[], Some("Studio PC"), 10).unwrap();
    assert_eq!(bundle.golden.len(), 3);
    // As if the username was removed on export: paths don't count.
    for g in &mut bundle.golden {
        for item in &mut g.manifest.items {
            item.path = PortablePath::Absolute("~/elsewhere".into());
        }
    }

    // Behind: has v1, gets v2 and v3 as its own v2 and v3.
    let mut behind = Store::open_in_memory().unwrap();
    behind.save_golden(&studio, None, None, 1).unwrap();
    let report = behind.import_bundle(&bundle, None, 20).unwrap();
    assert_eq!(report.golden, GoldenImport::Imported(vec![2, 3]));
    let latest = behind.golden(None).unwrap().unwrap();
    assert_eq!(latest.manifest.items, bundle.golden[2].manifest.items);
    assert_eq!(
        latest.info.note.as_deref(),
        Some("Imported: v3 from Studio PC")
    );

    // Ahead: its latest is newer than anything in an older bundle.
    let old = {
        let mut b = bundle.clone();
        b.golden.truncate(2);
        b
    };
    assert_eq!(
        behind.import_bundle(&old, None, 30).unwrap().golden,
        GoldenImport::UpToDate
    );

    // Diverged: both sides changed v1 differently. Nothing is imported.
    let mut other = studio.clone();
    other.items.remove(0);
    let mut diverged = Store::open_in_memory().unwrap();
    diverged.save_golden(&studio, None, None, 1).unwrap();
    diverged.save_golden(&other, None, None, 2).unwrap();
    assert_eq!(
        diverged.import_bundle(&bundle, None, 40).unwrap().golden,
        GoldenImport::Diverged
    );
    assert_eq!(diverged.golden(None).unwrap().unwrap().info.version, 2);

    // No Golden in the bundle.
    let empty = Bundle::new(1, None);
    assert_eq!(
        diverged.import_bundle(&empty, None, 50).unwrap().golden,
        GoldenImport::NotInBundle
    );
}

#[test]
fn import_never_overwrites_profiles() {
    let mut store = Store::open_in_memory().unwrap();
    let mut ours = laptop_profile();
    ours.name = "Our laptop".into();
    store.save_target_profile(&ours).unwrap();

    let mut bundle = Bundle::new(1, None);
    let travel = store.target_profile("travel").unwrap().unwrap();
    let mut fake_travel = travel.clone();
    fake_travel.rules.clear();
    bundle.profiles = vec![laptop_profile(), fake_travel];
    let report = store.import_bundle(&bundle, None, 2).unwrap();
    assert_eq!(report.profiles_conflicting, ["laptop", "travel"]);
    assert!(report.profiles_added.is_empty());
    assert_eq!(store.target_profile("laptop").unwrap().unwrap(), ours);
    assert_eq!(store.target_profile("travel").unwrap().unwrap(), travel);
}

#[test]
fn newer_bundles_are_refused() {
    let mut bundle = Bundle::new(1, None);
    bundle.bundle_version = BUNDLE_VERSION + 1;
    assert!(matches!(
        Bundle::from_json(&bundle.to_json_pretty()),
        Err(ManifestError::UnsupportedBundleVersion(v)) if v == BUNDLE_VERSION + 1
    ));

    let mut bundle = Bundle::new(1, None);
    let mut m = fixture_manifest();
    m.schema_version += 1;
    bundle.machines.push(BundleMachine {
        manifest: m,
        profile_id: None,
    });
    assert!(matches!(
        Bundle::from_json(&bundle.to_json_pretty()),
        Err(ManifestError::UnsupportedVersion(_))
    ));
}
