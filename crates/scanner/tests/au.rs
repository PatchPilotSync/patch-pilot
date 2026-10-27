use std::path::PathBuf;

use patchpilot_core::Confidence;
use patchpilot_scanner::read_au;

fn fixture(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/scanner/au")
        .join(rel)
}

#[test]
fn single_component() {
    let info = read_au(&fixture("single-component/Acme Verb.component"));
    assert_eq!(info.confidence, Confidence::High);
    assert_eq!(info.name, "Acme Verb");
    assert_eq!(info.version().as_deref(), Some("2.1"));
    assert_eq!(info.components.len(), 1);
    let c = &info.components[0];
    assert_eq!(c.component_id(), "aufx:Verb:Acme");
    assert_eq!(c.vendor.as_deref(), Some("Acme Audio"));
    assert_eq!(c.product_name, "Acme Verb");
    assert_eq!(c.version.as_deref(), Some("2.1.0"));
    assert_eq!(c.description.as_deref(), Some("Algorithmic reverb"));
    assert!(info.warnings.is_empty(), "{:?}", info.warnings);
}

#[test]
fn multiple_components_in_binary_plist() {
    let info = read_au(&fixture("multiple-components/Acme Synth.component"));
    assert_eq!(info.confidence, Confidence::High);
    let ids: Vec<_> = info.components.iter().map(|c| c.component_id()).collect();
    assert_eq!(ids, ["aumu:Syn1:Acme", "aumf:SyFx:Acme"]);
    assert_eq!(info.components[0].version.as_deref(), Some("4.0.2"));
    assert_eq!(info.components[1].version.as_deref(), Some("4.0.2"));
    assert_eq!(info.components[1].product_name, "Acme Synth FX");
    assert_eq!(info.warnings.len(), 1);
    assert!(
        info.warnings[0].contains("AudioComponents[2]"),
        "{:?}",
        info.warnings
    );
}

#[test]
fn legacy_bundle_without_components() {
    let info = read_au(&fixture("legacy-no-components/Old Phaser.component"));
    assert_eq!(info.confidence, Confidence::Medium);
    assert_eq!(info.name, "Old Phaser");
    assert_eq!(info.version().as_deref(), Some("1.0"));
    assert!(info.components.is_empty());
}

#[test]
fn missing_plist_is_low_confidence() {
    let info = read_au(&fixture("nope/Ghost.component"));
    assert_eq!(info.confidence, Confidence::Low);
    assert_eq!(info.name, "Ghost");
    assert_eq!(info.warnings.len(), 1);
}
