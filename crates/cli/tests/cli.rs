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
        "3.0 (Primary 3.0.1)",
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

/// Primary Rig commands against a DB seeded with two saved machines.
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
            vec!["--db".as_ref(), db.as_os_str(), "primary".as_ref()];
        all.extend(args.iter().map(std::ffi::OsStr::new));
        patchpilot(&all)
    };

    let (ok, _, err) = golden(&["show"]);
    assert!(!ok && err.contains("primary init"), "{err}");
    let (ok, _, err) = golden(&["promote", "--machine", "studio-pc"]);
    assert!(!ok && err.contains("primary init"), "{err}");

    let (ok, out, err) = golden(&["init", "--machine", "studio-pc"]);
    assert!(ok, "{err}");
    assert_eq!(out, "Created Primary Rig v1 from Studio PC\n");
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
        out.starts_with("Primary Rig v2 takes 1 product(s) from Travel MacBook"),
        "{out}"
    );
    let (ok, _, err) = golden(&["propose", "--from", "travel-macbook", "--product", "Nope"]);
    assert!(!ok && err.contains("\"Nope\""), "{err}");

    let (ok, out, err) = golden(&["promote", "--machine", "studio-pc"]);
    assert!(ok, "{err}");
    assert!(
        out.starts_with("Primary Rig v3 is now Studio PC's latest scan (was v2)"),
        "{out}"
    );
    let (ok, _, err) = golden(&["promote", "--machine", "studio-pc"]);
    assert!(!ok && err.contains("already matches v3"), "{err}");

    let (ok, out, err) = golden(&["history"]);
    assert!(ok, "{err}");
    let lines: Vec<_> = out.lines().collect();
    assert_eq!(lines[0], "3 Primary Rig version(s)");
    assert!(lines[1].trim_start().starts_with("Primary Rig v3"), "{out}");
    assert!(
        lines[2].contains("from Travel MacBook") && lines[2].ends_with("\"add the tuner\""),
        "{out}"
    );

    // v2 is still there and can be compared like any manifest.
    let file = dir.path().join("golden-v2.json");
    let (ok, out, err) = golden(&["show", "--version", "2", "--out", file.to_str().unwrap()]);
    assert!(ok, "{err}");
    assert!(out.starts_with("Primary Rig v2"), "{out}");
    assert!(out.contains("8 products"), "{out}");
    let (ok, out, err) = patchpilot(&[
        "diff".as_ref(),
        file.as_os_str(),
        fixtures("parity/diff/laptop-mac.json").as_os_str(),
        "--all".as_ref(),
    ]);
    assert!(ok, "{err}");
    assert!(out.contains("vs Primary Rig v2"), "{out}");
    let (ok, _, err) = golden(&["show", "--version", "9"]);
    assert!(!ok && err.contains("no Primary Rig v9"), "{err}");
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

/// Stored target profiles drive `diff` from the database and from files.
#[test]
fn profiles_drive_diff() {
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
    let run = |args: &[&str]| {
        let mut all: Vec<&std::ffi::OsStr> = vec!["--db".as_ref(), db.as_os_str()];
        all.extend(args.iter().map(std::ffi::OsStr::new));
        patchpilot(&all)
    };

    let (ok, out, err) = run(&["profile", "list"]);
    assert!(ok, "{err}");
    assert!(
        out.contains("studio-clone") && out.contains("Travel  (built-in)"),
        "{out}"
    );

    let (ok, _, err) = run(&["diff", "--machine", "Travel MacBook"]);
    assert!(!ok && err.contains("primary init"), "{err}");
    assert!(run(&["golden", "init", "--machine", "studio-pc"]).0);

    // Without a profile the laptop is marked down for the missing AAX.
    let (ok, out, err) = run(&["diff", "--machine", "Travel MacBook"]);
    assert!(ok, "{err}");
    assert!(
        out.contains("missing AAX") && !out.contains("profile:"),
        "{out}"
    );

    let (ok, out, err) = run(&["profile", "set", "travel macbook", "travel"]);
    assert!(ok, "{err}");
    assert_eq!(out, "Travel MacBook now uses profile travel\n");
    let (ok, out, _) = run(&["profile", "list"]);
    assert!(ok && out.contains("used by Travel MacBook"), "{out}");

    let (ok, out, err) = run(&["diff", "--machine", "travel-macbook"]);
    assert!(ok, "{err}");
    assert!(
        out.contains("profile: Travel") && !out.contains("missing AAX"),
        "{out}"
    );

    // File diffs pick up the stored profile of a known machine too.
    let golden = dir.path().join("golden.json");
    assert!(run(&["golden", "show", "--out", golden.to_str().unwrap()]).0);
    let laptop = fixtures("parity/diff/laptop-mac.json");
    let (ok, out, err) = run(&["diff", golden.to_str().unwrap(), laptop.to_str().unwrap()]);
    assert!(ok, "{err}");
    assert!(out.contains("profile: Travel"), "{out}");

    // Import a custom profile; built-ins are protected.
    let custom = dir.path().join("laptop.json");
    std::fs::write(
        &custom,
        r#"{"id":"no-vst2","name":"No VST2","rules":[{"action":"exclude","match":{"format":"vst2"}}]}"#,
    )
    .unwrap();
    let (ok, out, err) = run(&["profile", "import", custom.to_str().unwrap()]);
    assert!(ok, "{err}");
    assert_eq!(out, "Saved profile no-vst2 (1 rules)\n");
    let (ok, out, _) = run(&["profile", "show", "no-vst2"]);
    assert!(ok && out.contains("1. exclude format VST2"), "{out}");
    std::fs::write(&custom, r#"{"id":"travel","name":"Mine","rules":[]}"#).unwrap();
    let (ok, _, err) = run(&["profile", "import", custom.to_str().unwrap()]);
    assert!(!ok && err.contains("built-in"), "{err}");

    let (ok, out, _) = run(&["profile", "set", "travel-macbook", "none"]);
    assert!(ok && out.contains("whole Primary Rig"), "{out}");
    let (ok, _, err) = run(&["profile", "set", "nobody", "travel"]);
    assert!(!ok && err.contains("no saved machine"), "{err}");
}

#[test]
fn diff_reports_template_readiness() {
    let (ok, out, err) = patchpilot(&[
        "diff".as_ref(),
        fixtures("parity/readiness/golden.json").as_os_str(),
        fixtures("parity/readiness/laptop.json").as_os_str(),
    ]);
    assert!(ok, "{err}");
    for needle in [
        "1 plug-in your templates need is missing",
        "\nTemplates: 0 of 1 ready on Travel MacBook\n",
        "TrackTemplates/Vocals/Lead Vocal.RTrackTemplate  needs Acme Comp",
    ] {
        assert!(out.contains(needle), "{needle:?} not in:\n{out}");
    }
    // Both sides have the template itself.
    assert!(!out.contains("DAW file"), "{out}");
}

#[cfg(any(windows, target_os = "macos"))]
#[test]
fn scan_records_reaper_templates() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("rig.json");
    let (ok, stdout, err) = patchpilot(&[
        "scan".as_ref(),
        "--out".as_ref(),
        out.as_os_str(),
        "--no-default-folders".as_ref(),
        "--no-vendor-apps".as_ref(),
        "--reaper-dir".as_ref(),
        fixtures("parity/reaper/REAPER").as_os_str(),
    ]);
    assert!(ok, "{err}");
    assert!(stdout.contains(", 3 templates,"), "{stdout}");
    let m = patchpilot_core::Manifest::from_json(&std::fs::read_to_string(&out).unwrap()).unwrap();
    assert!(m.machine.path_map.roots.contains_key("REAPER_RESOURCES"));
    let lead = m
        .items
        .iter()
        .find(|i| {
            i.product_id.as_ref().is_some_and(|p| {
                p.0 == "reaper-template/tracktemplates-vocals-lead-vocal-rtracktemplate"
            })
        })
        .expect("lead vocal template");
    let req: Vec<_> = lead.requires.iter().map(|p| p.0.as_str()).collect();
    assert_eq!(req, ["acme-audio/acme-comp", "acme-audio/acme-verb"]);
    // Bus Glue's "Acme EQ (Mono)" normalizes like the scanner's VST2 names.
    assert!(
        m.items
            .iter()
            .any(|i| i.requires.contains(&"acme-audio/acme-eq".into())),
        "{:?}",
        m.items.iter().map(|i| &i.requires).collect::<Vec<_>>()
    );
}

#[test]
fn plan_prints_ordered_steps() {
    let (ok, out, err) = patchpilot(&[
        "plan".as_ref(),
        fixtures("parity/plan/golden.json").as_os_str(),
        fixtures("parity/plan/laptop.json").as_os_str(),
    ]);
    assert!(ok, "{err}");
    for needle in [
        "Bring New Laptop up to speed with Studio PC\n6 step(s).",
        "\nFoundation\n  1. Install iLok License Manager\n",
        "  2. Install Native Access  (after 1)\n",
        "\nVendor apps\n  3. Open Native Access and install 1 and update 1  (after 1, 2)\n",
        "       Kontakt 7 7.10.1 (you have 7.9.0)\n",
        "\nManual steps\n",
    ] {
        assert!(out.contains(needle), "{needle:?} not in:\n{out}");
    }

    let (ok, out, _) = patchpilot(&[
        "plan".as_ref(),
        fixtures("parity/plan/golden.json").as_os_str(),
        fixtures("parity/plan/golden.json").as_os_str(),
    ]);
    assert!(ok && out.contains("Nothing to do"), "{out}");
}

#[test]
fn export_and_import_a_bundle() {
    let dir = tempfile::tempdir().unwrap();
    let studio_db = dir.path().join("studio/patchpilot.db");
    let mac_db = dir.path().join("mac/patchpilot.db");
    {
        let mut store = patchpilot_store::Store::open(&studio_db).unwrap();
        for f in ["golden-studio-win.json", "laptop-mac.json"] {
            let text = std::fs::read_to_string(fixtures(&format!("parity/diff/{f}"))).unwrap();
            let m = patchpilot_core::Manifest::from_json(&text).unwrap();
            store
                .save_manifest(&m, patchpilot_store::SnapshotReason::FirstScan, 1)
                .unwrap();
        }
    }
    let run = |db: &PathBuf, args: &[&str]| {
        let mut all: Vec<&std::ffi::OsStr> = vec!["--db".as_ref(), db.as_os_str()];
        all.extend(args.iter().map(std::ffi::OsStr::new));
        patchpilot(&all)
    };
    let bundle = dir.path().join("rig-bundle.json");
    let bundle_s = bundle.to_str().unwrap();

    // No machine ID here and nothing else saved yet.
    let (ok, _, err) = run(&studio_db, &["export", "--out", bundle_s]);
    assert!(!ok && err.contains("nothing to export"), "{err}");

    let (ok, _, err) = run(&studio_db, &["golden", "init", "--machine", "studio-pc"]);
    assert!(ok, "{err}");
    let (ok, out, err) = run(
        &studio_db,
        &["export", "--out", bundle_s, "--machine", "Studio PC"],
    );
    assert!(ok, "{err}");
    assert!(
        out.contains("(1 machine(s), 1 Primary Rig version(s), 0 profile(s))"),
        "{out}"
    );

    // A fresh Mac gets the studio PC and the Primary Rig.
    let (ok, out, err) = run(&mac_db, &["import", bundle_s]);
    assert!(ok, "{err}");
    assert!(out.contains("New scans: Studio PC"), "{out}");
    assert!(out.contains("Primary Rig: added v1"), "{out}");
    let (ok, out, err) = run(&mac_db, &["golden", "history"]);
    assert!(ok, "{err}");
    assert!(out.contains("Imported: v1"), "{out}");

    let (ok, out, err) = run(&mac_db, &["import", bundle_s]);
    assert!(ok, "{err}");
    assert!(out.contains("Already up to date: Studio PC"), "{out}");
    assert!(out.contains("Primary Rig: already up to date"), "{out}");

    // Everything, then compare the Mac against the imported Primary Rig.
    let (ok, out, err) = run(&studio_db, &["export", "--out", bundle_s, "--all-machines"]);
    assert!(ok, "{err}");
    assert!(out.contains("(2 machine(s)"), "{out}");
    let (ok, out, err) = run(&mac_db, &["import", bundle_s]);
    assert!(ok, "{err}");
    assert!(out.contains("New scans: Travel MacBook"), "{out}");
    let (ok, out, err) = run(&mac_db, &["diff", "--machine", "Travel MacBook"]);
    assert!(ok, "{err}");
    assert!(out.contains("vs Primary Rig v1"), "{out}");

    // A bundle from a newer Patch Pilot is refused.
    let text = std::fs::read_to_string(&bundle).unwrap().replacen(
        "\"bundle_version\": 1",
        "\"bundle_version\": 99",
        1,
    );
    std::fs::write(&bundle, text).unwrap();
    let (ok, _, err) = run(&mac_db, &["import", bundle_s]);
    assert!(!ok && err.contains("99"), "{err}");
}
