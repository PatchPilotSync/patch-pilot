use std::path::PathBuf;

use patchpilot_core::{Arch, Confidence};
use patchpilot_scanner::{Vst3Source, read_vst3};

fn fixture(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/scanner/vst3")
        .join(rel)
}

#[test]
fn moduleinfo_present() {
    let info = read_vst3(&fixture("moduleinfo-present/Acme Verb.vst3"));
    assert_eq!(info.source, Vst3Source::ModuleInfo);
    assert_eq!(info.confidence, Confidence::High);
    assert_eq!(info.name, "Acme Verb");
    assert_eq!(info.vendor.as_deref(), Some("Acme Audio"));
    assert_eq!(info.version.as_deref(), Some("2.1.0"));
    assert_eq!(info.primary_cid(), Some("5653544163654D6163726F7665726200"));
    assert_eq!(info.classes.len(), 2);
    assert_eq!(info.classes[0].sub_categories, ["Fx", "Reverb"]);
    assert_eq!(info.bundle_id.as_deref(), Some("com.acmeaudio.verb.vst3"));
    assert!(info.warnings.is_empty(), "{:?}", info.warnings);
}

#[test]
fn plist_only() {
    let info = read_vst3(&fixture("plist-only/Acme Delay.vst3"));
    assert_eq!(info.source, Vst3Source::Plist);
    assert_eq!(info.confidence, Confidence::Medium);
    assert_eq!(info.name, "Acme Delay");
    assert_eq!(info.version.as_deref(), Some("1.4.2"));
    assert_eq!(info.vendor, None);
    assert!(info.classes.is_empty());
}

#[test]
fn pe_only_windows_bundle() {
    let info = read_vst3(&fixture("pe-only/Acme Comp.vst3"));
    assert_eq!(info.source, Vst3Source::PeResource);
    assert_eq!(info.confidence, Confidence::Medium);
    assert_eq!(info.name, "Acme Comp");
    assert_eq!(info.vendor.as_deref(), Some("Acme Audio"));
    assert_eq!(info.version.as_deref(), Some("3.0.1"));
    assert_eq!(info.arch, Some(Arch::X86_64));
    assert!(info.warnings.is_empty(), "{:?}", info.warnings);
}

#[test]
fn legacy_single_file() {
    let info = read_vst3(&fixture("legacy-single-file/Acme Gate.vst3"));
    assert_eq!(info.source, Vst3Source::PeResource);
    assert_eq!(info.name, "Acme Gate");
    assert_eq!(info.version.as_deref(), Some("1.2"));
}

#[test]
fn name_only_is_low_confidence() {
    let info = read_vst3(&fixture("name-only/Mystery Synth.vst3"));
    assert_eq!(info.source, Vst3Source::FileName);
    assert_eq!(info.confidence, Confidence::Low);
    assert_eq!(info.name, "Mystery Synth");
    assert_eq!(info.version, None);
}

#[test]
fn broken_moduleinfo_falls_back_to_pe() {
    let info = read_vst3(&fixture("broken-moduleinfo/Acme Chorus.vst3"));
    assert_eq!(info.source, Vst3Source::PeResource);
    assert_eq!(info.version.as_deref(), Some("0.9.0"));
    assert_eq!(info.warnings.len(), 1);
    assert!(
        info.warnings[0].contains("moduleinfo.json"),
        "{:?}",
        info.warnings
    );
}

#[test]
fn missing_path_degrades_to_name() {
    let info = read_vst3(&fixture("nope/Ghost.vst3"));
    assert_eq!(info.confidence, Confidence::Low);
    assert_eq!(info.name, "Ghost");
    assert_eq!(info.warnings.len(), 1);
}
