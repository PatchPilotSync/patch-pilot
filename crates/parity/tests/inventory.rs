use std::path::PathBuf;

use patchpilot_core::{Confidence, ItemKind, Manifest, PluginFormat};
use patchpilot_parity::inventory;

fn load(rel: &str) -> Manifest {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(rel);
    Manifest::from_json(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn groups_plugins_by_vendor_with_counts() {
    let inv = inventory(&load("parity/plan/golden.json"));
    assert_eq!((inv.plugins, inv.products, inv.templates), (7, 6, 1));
    let formats: Vec<_> = inv.formats.iter().map(|f| (f.format, f.count)).collect();
    assert_eq!(formats, [(PluginFormat::Vst3, 6), (PluginFormat::Aax, 1)]);
    let vendors: Vec<_> = inv.vendors.iter().map(|v| v.vendor.as_str()).collect();
    assert_eq!(
        vendors,
        ["Acme Audio", "FabFilter", "Native Instruments", "Waves"]
    );
    let verb = &inv.vendors[0].products[1];
    assert_eq!(verb.name, "Acme Verb");
    assert_eq!(verb.formats, [PluginFormat::Vst3, PluginFormat::Aax]);
    let apps: Vec<_> = inv.apps.iter().map(|a| (a.name.as_str(), a.kind)).collect();
    assert_eq!(
        apps,
        [
            ("iLok License Manager", ItemKind::LicenseTool),
            ("Native Access", ItemKind::VendorApp),
            ("PACE License Support", ItemKind::LicenseTool),
            ("Waves Central", ItemKind::VendorApp),
        ]
    );
}

#[test]
fn newest_version_weakest_confidence_unknown_vendor_last() {
    let mut m = load("parity/plan/golden.json");
    let verb = m
        .items
        .iter()
        .find(|i| i.format == Some(PluginFormat::Vst3) && i.version.as_deref() == Some("2.1.0"))
        .unwrap()
        .clone();
    let mut old_vst2 = verb.clone();
    old_vst2.format = Some(PluginFormat::Vst2);
    old_vst2.version = Some("2.0".into());
    old_vst2.confidence = Confidence::Low;
    m.items.push(old_vst2);

    let id = patchpilot_core::ProductId("unknown/mystery".into());
    m.products.push(patchpilot_core::Product {
        canonical_id: id.clone(),
        vendor: String::new(),
        name: "Mystery".into(),
    });
    let mut mystery = verb;
    mystery.product_id = Some(id);
    m.items.push(mystery);

    let inv = inventory(&m);
    let row = &inv.vendors[0].products[1];
    assert_eq!(
        row.formats,
        [PluginFormat::Vst3, PluginFormat::Aax, PluginFormat::Vst2]
    );
    assert_eq!(row.version.as_deref(), Some("2.1.0"));
    assert_eq!(row.confidence, Confidence::Low);
    let last = inv.vendors.last().unwrap();
    assert_eq!(last.vendor, "");
    assert_eq!(last.products[0].name, "Mystery");
}
