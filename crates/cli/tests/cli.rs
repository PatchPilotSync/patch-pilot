use std::path::PathBuf;
use std::process::Command;

fn fixtures(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(rel)
}

fn patchpilot(args: &[&std::ffi::OsStr]) -> (bool, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_patchpilot"))
        .args(args)
        .output()
        .unwrap();
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn diff_with_travel_profile_is_100() {
    let (ok, out, err) = patchpilot(&[
        "diff".as_ref(),
        fixtures("parity/score/golden.json").as_os_str(),
        fixtures("parity/score/laptop.json").as_os_str(),
        "--profile".as_ref(),
        fixtures("parity/score/travel-profile.json").as_os_str(),
    ]);
    assert!(ok, "{err}");
    assert!(out.starts_with("Parity 100%"), "{out}");
    assert!(out.contains("profile: Travel"), "{out}");
}

#[test]
fn diff_lists_states_and_reasons() {
    let (ok, out, err) = patchpilot(&[
        "diff".as_ref(),
        fixtures("parity/diff/golden-studio-win.json").as_os_str(),
        fixtures("parity/diff/laptop-mac.json").as_os_str(),
    ]);
    assert!(ok, "{err}");
    for needle in [
        "1 plug-in is missing",
        "\nMissing\n  Acme Gate",
        "\nOut of date\n  Acme Comp",
        "3.0 (Golden 3.0.1)",
        "missing AAX",
        "1 item(s) on Travel MacBook couldn't be identified",
    ] {
        assert!(out.contains(needle), "{needle:?} not in:\n{out}");
    }
    assert!(
        !out.contains("Acme Synth"),
        "in-parity hidden without --all:\n{out}"
    );
}

#[test]
fn diff_reports_bad_input() {
    let (ok, _, err) = patchpilot(&["diff".as_ref(), "nope.json".as_ref(), "nope.json".as_ref()]);
    assert!(!ok);
    assert!(err.starts_with("patchpilot: nope.json"), "{err}");
}

#[cfg(any(windows, target_os = "macos"))]
#[test]
fn scan_fixture_folders() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("rig.json");
    let folder = |f: &str, p: &str| format!("{f}={}", fixtures(p).display());
    let (ok, stdout, err) = patchpilot(&[
        "scan".as_ref(),
        "--out".as_ref(),
        out.as_os_str(),
        "--name".as_ref(),
        "Test Rig".as_ref(),
        "--no-default-folders".as_ref(),
        "--no-vendor-apps".as_ref(),
        "--folder".as_ref(),
        folder("vst3", "scanner/vst3/pe-only").as_ref(),
        "--folder".as_ref(),
        folder("vst2", "scanner/vst2-aax/windows").as_ref(),
    ]);
    assert!(ok, "{err}");
    assert!(stdout.starts_with("Wrote "), "{stdout}");
    let m = patchpilot_core::Manifest::from_json(&std::fs::read_to_string(&out).unwrap()).unwrap();
    assert_eq!(m.machine.name, "Test Rig");
    let ids: Vec<_> = m
        .products
        .iter()
        .map(|p| p.canonical_id.0.as_str())
        .collect();
    assert!(ids.contains(&"acme-audio/acme-comp"), "{ids:?}");
    assert!(ids.contains(&"acme-audio/acme-verb"), "{ids:?}");
}

/// Golden Rig commands against a DB seeded with two saved machines.
#[test]
fn golden_init_propose_promote_and_history() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("patchpilot.db");
    {
        let mut store = patchpilot_store::Store::open(&db).unwrap();
        for f in ["golden-studio-win.json", "laptop-mac.json"] {
            let text = std::fs::read_to_string(fixtures(&format!("parity/diff/{f}"))).unwrap();
            let m = patchpilot_core::Manifest::from_json(&text).unwrap();
            store
                .save_manifest(&m, patchpilot_store::SnapshotReason::FirstScan, 1)
                .unwrap();
        }
    }
    let golden = |args: &[&str]| {
        let mut all: Vec<&std::ffi::OsStr> =
            vec!["--db".as_ref(), db.as_os_str(), "golden".as_ref()];
        all.extend(args.iter().map(std::ffi::OsStr::new));
        patchpilot(&all)
    };

    let (ok, _, err) = golden(&["show"]);
    assert!(!ok && err.contains("golden init"), "{err}");
    let (ok, _, err) = golden(&["promote", "--machine", "studio-pc"]);
    assert!(!ok && err.contains("golden init"), "{err}");

    let (ok, out, err) = golden(&["init", "--machine", "studio-pc"]);
    assert!(ok, "{err}");
    assert_eq!(out, "Created Golden Rig v1 from Studio PC\n");
    let (ok, _, err) = golden(&["init", "--machine", "studio-pc"]);
    assert!(!ok && err.contains("already exists (v1)"), "{err}");

    let (ok, out, err) = golden(&[
        "propose",
        "--from",
        "travel-macbook",
        "--product",
        "Acme Tuner",
        "--note",
        "add the tuner",
    ]);
    assert!(ok, "{err}");
    assert!(
        out.starts_with("Golden Rig v2 takes 1 product(s) from Travel MacBook"),
        "{out}"
    );
    let (ok, _, err) = golden(&["propose", "--from", "travel-macbook", "--product", "Nope"]);
    assert!(!ok && err.contains("\"Nope\""), "{err}");

    let (ok, out, err) = golden(&["promote", "--machine", "studio-pc"]);
    assert!(ok, "{err}");
    assert!(
        out.starts_with("Golden Rig v3 is now Studio PC's latest scan (was v2)"),
        "{out}"
    );
    let (ok, _, err) = golden(&["promote", "--machine", "studio-pc"]);
    assert!(!ok && err.contains("already matches v3"), "{err}");

    let (ok, out, err) = golden(&["history"]);
    assert!(ok, "{err}");
    let lines: Vec<_> = out.lines().collect();
    assert_eq!(lines[0], "3 Golden Rig version(s)");
    assert!(lines[1].trim_start().starts_with("Golden Rig v3"), "{out}");
    assert!(
        lines[2].contains("from Travel MacBook") && lines[2].ends_with("\"add the tuner\""),
        "{out}"
    );

    // v2 is still there and can be compared like any manifest.
    let file = dir.path().join("golden-v2.json");
    let (ok, out, err) = golden(&["show", "--version", "2", "--out", file.to_str().unwrap()]);
    assert!(ok, "{err}");
    assert!(out.starts_with("Golden Rig v2"), "{out}");
    assert!(out.contains("8 products"), "{out}");
    let (ok, out, err) = patchpilot(&[
        "diff".as_ref(),
        file.as_os_str(),
        fixtures("parity/diff/laptop-mac.json").as_os_str(),
        "--all".as_ref(),
    ]);
    assert!(ok, "{err}");
    assert!(out.contains("vs Golden Golden Rig v2"), "{out}");
    let (ok, _, err) = golden(&["show", "--version", "9"]);
    assert!(!ok && err.contains("no Golden Rig v9"), "{err}");
}

#[test]
fn scan_needs_out_or_save() {
    let (ok, _, err) = patchpilot(&["scan".as_ref()]);
    assert!(!ok);
    assert!(err.contains("--out") && err.contains("--save"), "{err}");
}

#[test]
fn history_before_any_saved_scan() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("patchpilot.db");
    let (ok, _, err) = patchpilot(&["history".as_ref(), "--db".as_ref(), db.as_os_str()]);
    assert!(!ok);
    assert!(err.contains("no saved scans"), "{err}");
    assert!(!db.exists(), "history must not create a database");
}

/// Two saved scans of an unchanged tree share one snapshot and its item
/// rows; removing a plug-in makes a second snapshot that reuses the rest.
#[cfg(any(windows, target_os = "macos"))]
#[test]
fn saved_scans_build_history() {
    let dir = tempfile::tempdir().unwrap();
    let app = dir.path().join("PatchPilot");
    let db = app.join("patchpilot.db");
    let plugins = dir.path().join("VST");
    std::fs::create_dir(&plugins).unwrap();
    for f in std::fs::read_dir(fixtures("scanner/vst2-aax/windows")).unwrap() {
        let f = f.unwrap().path();
        if f.extension().is_some_and(|e| e == "dll") {
            std::fs::copy(&f, plugins.join(f.file_name().unwrap())).unwrap();
        }
    }
    let folder = format!("vst2={}", plugins.display());
    let scan = |name: Option<&str>| {
        let mut args: Vec<&std::ffi::OsStr> = vec![
            "scan".as_ref(),
            "--save".as_ref(),
            "--db".as_ref(),
            db.as_os_str(),
            "--no-default-folders".as_ref(),
            "--no-vendor-apps".as_ref(),
            "--folder".as_ref(),
            folder.as_ref(),
        ];
        if let Some(n) = name {
            args.extend::<[&std::ffi::OsStr; 2]>(["--name".as_ref(), n.as_ref()]);
        }
        let (ok, out, err) = patchpilot(&args);
        assert!(ok, "{err}");
        out
    };

    let out = scan(Some("Test Rig"));
    assert!(out.starts_with("Saved snapshot #1 of Test Rig"), "{out}");
    let machine_id = std::fs::read_to_string(app.join("machine-id")).unwrap();
    // No --name: the saved name is kept.
    let out = scan(None);
    assert!(out.starts_with("No changes since snapshot #1"), "{out}");

    std::fs::remove_file(plugins.join("Acme Gate (64 bit).dll")).unwrap();
    let out = scan(None);
    assert!(out.starts_with("Saved snapshot #2 of Test Rig"), "{out}");
    assert_eq!(
        std::fs::read_to_string(app.join("machine-id")).unwrap(),
        machine_id
    );

    let store = patchpilot_store::Store::open(&db).unwrap();
    let rows: i64 = store
        .connection()
        .query_row("SELECT count(*) FROM items", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rows, 3, "unchanged items are shared between snapshots");
    drop(store);

    let (ok, out, err) = patchpilot(&["history".as_ref(), "--db".as_ref(), db.as_os_str()]);
    assert!(ok, "{err}");
    assert!(out.starts_with("2 snapshot(s) of Test Rig\n"), "{out}");
    let lines: Vec<_> = out.lines().collect();
    assert!(lines[1].trim_start().starts_with("#2"), "{out}");
    assert!(
        lines[1].contains("scan") && lines[1].ends_with("+0 -1"),
        "{out}"
    );
    assert!(lines[2].contains("first scan"), "{out}");
}
