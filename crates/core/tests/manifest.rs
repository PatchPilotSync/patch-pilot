use std::path::PathBuf;

use patchpilot_core::{
    Arch, Confidence, Item, MachineInfo, Manifest, ManifestError, Os, PathMap, PluginFormat,
    PortablePath, Product,
};

fn workspace_file(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel)
}

fn fixture() -> String {
    std::fs::read_to_string(workspace_file("fixtures/core/manifest-v1/studio-pc.json")).unwrap()
}

#[test]
fn fixture_round_trips() {
    let manifest = Manifest::from_json(&fixture()).unwrap();
    let again = Manifest::from_json(&manifest.to_json_pretty()).unwrap();
    assert_eq!(manifest, again);

    let as_value = |s: &str| serde_json::from_str::<serde_json::Value>(s).unwrap();
    assert_eq!(as_value(&fixture()), as_value(&manifest.to_json_pretty()));
}

#[test]
fn fixture_reads_as_expected() {
    let m = Manifest::from_json(&fixture()).unwrap();
    assert_eq!(m.machine.os, Os::Windows);
    assert_eq!(m.items.len(), 4);

    let verb = &m.items[0];
    assert_eq!(verb.format, Some(PluginFormat::Vst3));
    assert_eq!(verb.confidence, Confidence::High);
    assert_eq!(
        m.product(verb.product_id.as_ref().unwrap()).unwrap().name,
        "Acme Verb"
    );
    assert_eq!(
        m.machine.path_map.resolve(&verb.path).unwrap(),
        r"C:\Program Files\Common Files\VST3\Acme\Acme Verb.vst3"
    );
    assert!(m.items[3].product_id.is_none());
}

#[test]
fn builds_and_round_trips_a_macos_manifest() {
    let path_map = PathMap::new(Os::Macos).with_root("VST3_SYSTEM", "/Library/Audio/Plug-Ins/VST3");
    let mut m = Manifest::new(
        MachineInfo {
            id: "b1".into(),
            name: "Travel MacBook".into(),
            os: Os::Macos,
            arch: Arch::Aarch64,
            os_version: None,
            path_map: path_map.clone(),
            disks: Vec::new(),
        },
        1_792_771_200,
    );
    m.products.push(Product {
        canonical_id: "vst3:5653544163654D6163726F7665726200".into(),
        vendor: "Acme Audio".into(),
        name: "Acme Verb".into(),
    });
    let mut item = Item::plugin(
        PluginFormat::Vst3,
        Os::Macos,
        path_map.to_portable("/Library/Audio/Plug-Ins/VST3/Acme/Acme Verb.vst3"),
    );
    item.product_id = Some("vst3:5653544163654D6163726F7665726200".into());
    m.items.push(item);

    // Same logical path as the Windows fixture: this is what makes manifests comparable.
    assert_eq!(
        m.items[0].path,
        PortablePath::logical("VST3_SYSTEM", "Acme/Acme Verb.vst3")
    );
    assert_eq!(Manifest::from_json(&m.to_json_pretty()).unwrap(), m);
}

#[test]
fn rejects_newer_schema_versions() {
    let newer = fixture().replacen("\"schema_version\": 1", "\"schema_version\": 2", 1);
    assert!(matches!(
        Manifest::from_json(&newer),
        Err(ManifestError::UnsupportedVersion(2))
    ));
}

#[test]
fn rejects_malformed_hashes() {
    let bad = fixture().replacen("blake3:af13", "sha1:af13", 1);
    let err = Manifest::from_json(&bad).unwrap_err();
    assert!(err.to_string().contains("blake3"), "{err}");
}

/// `docs/manifest.schema.json` must match the types. Regenerate with
/// `UPDATE_SCHEMA=1 cargo test -p patchpilot-core`.
#[test]
fn checked_in_json_schema_is_current() {
    let path = workspace_file("docs/manifest.schema.json");
    let generated =
        serde_json::to_string_pretty(&patchpilot_core::manifest_json_schema()).unwrap() + "\n";
    if std::env::var_os("UPDATE_SCHEMA").is_some() {
        std::fs::write(&path, &generated).unwrap();
    }
    let checked_in = std::fs::read_to_string(&path)
        .unwrap_or_default()
        .replace("\r\n", "\n");
    assert!(
        checked_in == generated,
        "docs/manifest.schema.json is stale; run UPDATE_SCHEMA=1 cargo test -p patchpilot-core"
    );
}
