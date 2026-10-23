use std::path::PathBuf;

use patchpilot_core::{Arch, Confidence, Os, PluginFormat, PortablePath};
use patchpilot_scanner::{ClapCache, PluginRoot, ScanInput, scan_machine};

fn fixture(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/scanner")
        .join(rel)
}

#[test]
fn scans_fixture_roots_into_a_manifest() {
    let out = scan_machine(ScanInput {
        id: "test".into(),
        name: "Test".into(),
        os: Os::Windows,
        arch: Arch::X86_64,
        os_version: None,
        roots: vec![
            PluginRoot::new("VST3_SYSTEM", PluginFormat::Vst3, fixture("vst3/pe-only")),
            PluginRoot::new(
                "VST2_PROGRAM_FILES",
                PluginFormat::Vst2,
                fixture("vst2-aax/windows"),
            ),
        ],
        vendor_apps: Vec::new(),
        clap_cache: ClapCache::default(),
        now: 1_800_000_000,
    });
    let m = out.manifest;
    assert_eq!(m.generated_at, 1_800_000_000);

    let comp: Vec<_> = m
        .items
        .iter()
        .filter(|i| {
            i.product_id
                .as_ref()
                .is_some_and(|p| p.0 == "acme-audio/acme-comp")
        })
        .collect();
    // VST3 bundle and VST2 DLL of the same product share one ID.
    let mut formats: Vec<_> = comp
        .iter()
        .map(|i| format!("{:?}", i.format.unwrap()))
        .collect();
    formats.sort();
    assert_eq!(formats, ["Vst2", "Vst3"]);
    assert!(comp.iter().all(|i| i.confidence == Confidence::Medium));

    let vst3 = comp
        .iter()
        .find(|i| i.format == Some(PluginFormat::Vst3))
        .unwrap();
    assert_eq!(
        vst3.path,
        PortablePath::logical("VST3_SYSTEM", "Acme Comp.vst3")
    );
    assert_eq!(vst3.version.as_deref(), Some("3.0.1"));

    // No vendor in a PE without a version resource.
    assert!(m.product(&"unknown/acme-gate".into()).is_some());
    let verb = m.product(&"acme-audio/acme-verb".into()).unwrap();
    assert_eq!(verb.name, "Acme Verb");
    // One product entry per ID.
    let mut ids: Vec<_> = m.products.iter().map(|p| &p.canonical_id).collect();
    let n = ids.len();
    ids.dedup();
    assert_eq!(ids.len(), n);
}
