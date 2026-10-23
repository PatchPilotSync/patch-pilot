use std::collections::BTreeMap;
use std::path::PathBuf;

use patchpilot_core::ItemKind;
use patchpilot_scanner::{
    ADAPTERS, Detection, UninstallEntry, VendorAppStatus, detect_macos, detect_windows,
};

fn fixture(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/scanner/vendor-apps")
        .join(rel)
}

/// adapter id -> Some(version) if detected (version may be ""), None if not.
fn by_id(statuses: Vec<VendorAppStatus>) -> BTreeMap<&'static str, Option<String>> {
    assert_eq!(statuses.len(), ADAPTERS.len(), "every adapter reports");
    statuses
        .into_iter()
        .map(|s| {
            let v = match s.detection {
                Detection::Detected { version, .. } => Some(version.unwrap_or_default()),
                Detection::NotDetected => None,
            };
            (s.adapter.id, v)
        })
        .collect()
}

#[test]
fn windows_uninstall_entries() {
    let json = std::fs::read_to_string(fixture("windows/uninstall.json")).unwrap();
    let entries: Vec<UninstallEntry> = serde_json::from_str(&json).unwrap();
    let found = by_id(detect_windows(&entries));

    assert_eq!(found["native-access"].as_deref(), Some("2.6.1"));
    assert_eq!(found["waves-central"].as_deref(), Some("15.3.0"));
    assert_eq!(found["ilok-license-manager"].as_deref(), Some("5.9.1"));
    assert_eq!(found["pace-license-support"].as_deref(), Some("5.9.1.1913"));
    // Detected without a version.
    assert_eq!(found["arturia-software-center"].as_deref(), Some(""));
    // Publisher doesn't match.
    assert_eq!(found["ua-connect"], None);
    assert_eq!(found["spitfire-audio"], None);
    assert_eq!(found["izotope-product-portal"], None);
}

#[test]
fn windows_location_falls_back_to_name() {
    let json = std::fs::read_to_string(fixture("windows/uninstall.json")).unwrap();
    let entries: Vec<UninstallEntry> = serde_json::from_str(&json).unwrap();
    let statuses = detect_windows(&entries);
    let loc = |id| match &statuses
        .iter()
        .find(|s| s.adapter.id == id)
        .unwrap()
        .detection
    {
        Detection::Detected { location, .. } => location.clone(),
        Detection::NotDetected => None,
    };
    assert_eq!(
        loc("native-access").as_deref(),
        Some(r"C:\Program Files\Native Instruments\Native Access\")
    );
    assert_eq!(
        loc("ilok-license-manager").as_deref(),
        Some("iLok License Manager")
    );
}

#[test]
fn macos_bundles() {
    let found = by_id(detect_macos(&[
        fixture("macos/root"),
        fixture("macos/home"),
    ]));

    assert_eq!(found["native-access"].as_deref(), Some("3.10.0"));
    assert_eq!(found["izotope-product-portal"].as_deref(), Some("1.6.2"));
    assert_eq!(found["ilok-license-manager"].as_deref(), Some("5.9.1"));
    assert_eq!(found["ua-connect"].as_deref(), Some("1.4.0"));
    // Broken plist: installed, version unknown.
    assert_eq!(found["spitfire-audio"].as_deref(), Some(""));
    // Presence-only helper.
    assert_eq!(found["pace-license-support"].as_deref(), Some(""));
    assert_eq!(found["waves-central"], None);
}

#[test]
fn nothing_installed_is_all_not_detected() {
    assert!(by_id(detect_windows(&[])).values().all(Option::is_none));
    assert!(
        by_id(detect_macos(&[fixture("does-not-exist")]))
            .values()
            .all(Option::is_none)
    );
}

#[test]
fn licensing_tools_are_marked() {
    for a in ADAPTERS {
        let is_license = a.id.starts_with("ilok") || a.id.starts_with("pace");
        let expected = if is_license {
            ItemKind::LicenseTool
        } else {
            ItemKind::VendorApp
        };
        assert_eq!(a.kind, expected, "{}", a.id);
    }
}
