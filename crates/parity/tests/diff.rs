use std::path::PathBuf;

use patchpilot_core::{Manifest, PluginFormat};
use patchpilot_parity::{ParityState::*, diff_manifests};

fn load(name: &str) -> Manifest {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/parity/diff")
        .join(name);
    Manifest::from_json(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn windows_golden_vs_mac_laptop() {
    let golden = load("golden-studio-win.json");
    let laptop = load("laptop-mac.json");
    let diff = diff_manifests(&golden, &laptop);

    let table = [
        ("acme-verb", Incomplete),
        ("acme-comp", Outdated),
        ("acme-synth", InParity),
        ("acme-gate", Missing),
        ("acme-delay", NewerThanGolden),
        ("acme-gate-jsfx", Modified),
        ("acme-phaser", VersionUnknown),
        ("acme-tuner", Extra),
    ];
    for (id, state) in table {
        let p = diff
            .products
            .iter()
            .find(|p| p.product_id.0 == id)
            .unwrap_or_else(|| panic!("{id} missing from diff"));
        assert_eq!(p.state, state, "{id}: {p:?}");
    }
    assert_eq!(diff.products.len(), table.len());

    let verb = diff
        .products
        .iter()
        .find(|p| p.product_id.0 == "acme-verb")
        .unwrap();
    assert_eq!(verb.missing_formats, [PluginFormat::Aax]);
    let comp = diff
        .products
        .iter()
        .find(|p| p.product_id.0 == "acme-comp")
        .unwrap();
    assert_eq!(
        (
            comp.golden_version.as_deref(),
            comp.machine_version.as_deref()
        ),
        (Some("3.0.1"), Some("3.0"))
    );
    // The item with no product_id is reported, not guessed.
    assert_eq!(diff.unresolved.len(), 1);
    assert!(laptop.items[diff.unresolved[0]].product_id.is_none());
}

#[test]
fn sorted_most_urgent_first() {
    let diff = diff_manifests(&load("golden-studio-win.json"), &load("laptop-mac.json"));
    let states: Vec<_> = diff.products.iter().map(|p| p.state).collect();
    let mut sorted = states.clone();
    sorted.sort();
    assert_eq!(states, sorted);
    assert_eq!(diff.products[0].state, Missing);
}

#[test]
fn au_only_product_is_not_applicable_on_windows() {
    // Reverse direction: the Mac is Golden, the PC is measured.
    let diff = diff_manifests(&load("laptop-mac.json"), &load("golden-studio-win.json"));
    let tuner = diff
        .products
        .iter()
        .find(|p| p.product_id.0 == "acme-tuner")
        .unwrap();
    assert_eq!(tuner.state, NotApplicable);
    // ...while its cross-platform VST3s are still compared.
    let comp = diff
        .products
        .iter()
        .find(|p| p.product_id.0 == "acme-comp")
        .unwrap();
    assert_eq!(comp.state, NewerThanGolden);
    // AU isn't expected on Windows, so Acme Verb is complete there.
    let verb = diff
        .products
        .iter()
        .find(|p| p.product_id.0 == "acme-verb")
        .unwrap();
    assert_eq!(verb.state, InParity);
    assert!(verb.missing_formats.is_empty());
}

#[test]
fn identical_manifests_are_in_parity() {
    let golden = load("golden-studio-win.json");
    let diff = diff_manifests(&golden, &golden);
    assert!(
        diff.products.iter().all(|p| p.state == InParity),
        "{diff:?}"
    );
}
