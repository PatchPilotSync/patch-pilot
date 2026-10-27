use std::path::PathBuf;

use patchpilot_core::{Arch, Confidence, PluginFormat};
use patchpilot_scanner::{Channels, MetaSource, read_aax, read_vst2};

fn fixture(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/scanner/vst2-aax")
        .join(rel)
}

#[test]
fn windows_vst2_dll() {
    let m = read_vst2(&fixture("windows/Acme Comp x64.dll"));
    assert_eq!(m.format, PluginFormat::Vst2);
    assert_eq!(m.source, MetaSource::PeResource);
    assert_eq!(m.confidence, Confidence::Medium);
    assert_eq!(m.name, "Acme Comp");
    assert_eq!(m.vendor.as_deref(), Some("Acme Audio"));
    assert_eq!(m.version.as_deref(), Some("5.2.1"));
    assert_eq!(m.arch, Some(Arch::X86_64));
    assert!(m.warnings.is_empty(), "{:?}", m.warnings);
}

#[test]
fn channel_tag_from_file_name() {
    let m = read_vst2(&fixture("windows/Acme EQ Mono.dll"));
    assert_eq!(m.name, "Acme EQ");
    assert_eq!(m.channels, Some(Channels::Mono));
}

#[test]
fn dll_without_version_resource() {
    let m = read_vst2(&fixture("windows/Acme Gate (64 bit).dll"));
    assert_eq!(m.source, MetaSource::FileName);
    assert_eq!(m.confidence, Confidence::Low);
    assert_eq!(m.name, "Acme Gate");
    assert_eq!(m.raw_name, "Acme Gate (64 bit)");
    assert_eq!(m.arch, Some(Arch::X86_64));
    // No version resource is normal for many vendors, so it isn't a warning.
    assert!(m.warnings.is_empty(), "{:?}", m.warnings);
    assert_eq!(m.version, None);
}

#[test]
fn macos_vst2_bundle() {
    let m = read_vst2(&fixture("macos/Acme Comp.vst"));
    assert_eq!(m.source, MetaSource::Plist);
    assert_eq!(m.name, "Acme Comp");
    assert_eq!(m.version.as_deref(), Some("5.2.1"));
    assert_eq!(m.bundle_id.as_deref(), Some("com.acmeaudio.comp.vst"));
}

#[test]
fn windows_aax_bundle() {
    let m = read_aax(&fixture("windows/Acme Verb.aaxplugin"));
    assert_eq!(m.format, PluginFormat::Aax);
    assert_eq!(m.source, MetaSource::PeResource);
    assert_eq!(m.name, "Acme Verb");
    assert_eq!(m.vendor.as_deref(), Some("Acme Audio"));
    assert_eq!(m.version.as_deref(), Some("2.1.0"));
}

#[test]
fn macos_aax_bundle() {
    let m = read_aax(&fixture("macos/Acme Verb.aaxplugin"));
    assert_eq!(m.source, MetaSource::Plist);
    assert_eq!(m.name, "Acme Verb");
    assert_eq!(m.version.as_deref(), Some("2.1.0"));
}

#[test]
fn same_product_on_both_oses_has_the_same_name() {
    let win = read_vst2(&fixture("windows/Acme Comp x64.dll"));
    let mac = read_vst2(&fixture("macos/Acme Comp.vst"));
    assert_eq!((win.name, win.version), (mac.name, mac.version));
}
