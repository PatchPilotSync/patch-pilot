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
