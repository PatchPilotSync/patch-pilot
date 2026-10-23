#[cfg(any(windows, target_os = "macos"))]
fn fixtures(rel: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(rel)
}

#[test]
fn extra_roots_parse() {
    let r = patchpilot_local::extra_root(2, "VST3=D:\\Plugins").unwrap();
    assert_eq!(r.format, patchpilot_core::PluginFormat::Vst3);
    assert_eq!(r.name, "EXTRA_2");
    assert!(patchpilot_local::extra_root(1, "D:\\Plugins").is_err());
    assert!(patchpilot_local::extra_root(1, "midi=D:\\x").is_err());
}

// Scanning needs a supported OS; CI runs this on Windows and macOS.
#[cfg(any(windows, target_os = "macos"))]
#[test]
fn scan_save_and_summarize_a_fixture_tree() {
    use patchpilot_local::{ScanOptions, Stage, save_scan, scan_this_machine};

    let opts = ScanOptions {
        default_folders: false,
        vendor_apps: false,
        extra_roots: vec![
            patchpilot_local::extra_root(
                1,
                &format!("vst3={}", fixtures("scanner/vst3/pe-only").display()),
            )
            .unwrap(),
            patchpilot_local::extra_root(
                2,
                &format!("vst2={}", fixtures("scanner/vst2-aax/windows").display()),
            )
            .unwrap(),
        ],
        reaper_dir: Some(fixtures("parity/reaper/REAPER")),
        ..ScanOptions::standard("0123456789abcdef".into(), "Test Rig".into())
    };
    let mut stages = Vec::new();
    let scan = scan_this_machine(opts, |s| stages.push(s)).unwrap();
    assert_eq!(
        stages,
        [Stage::VendorApps, Stage::Plugins, Stage::Templates]
    );
    assert!(scan.plugins() >= 2, "{:?}", scan.manifest.items);
    assert!(scan.templates >= 1);

    let inv = patchpilot_parity::inventory(&scan.manifest);
    assert_eq!(inv.plugins, scan.plugins());
    assert!(inv.vendors.iter().any(|v| v.vendor == "Acme Audio"));

    let dir = tempfile::tempdir().unwrap();
    let mut store = patchpilot_store::Store::open(&dir.path().join("patchpilot.db")).unwrap();
    let first = save_scan(&mut store, &scan.manifest).unwrap();
    assert!(!first.unchanged);
    let again = save_scan(&mut store, &scan.manifest).unwrap();
    assert!(again.unchanged);
    let history = store.snapshots("0123456789abcdef").unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(
        history[0].reason,
        patchpilot_store::SnapshotReason::FirstScan
    );
}
