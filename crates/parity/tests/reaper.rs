use std::path::PathBuf;

use patchpilot_core::PluginFormat::*;
use patchpilot_parity::reaper::{find_templates, scan_templates};

fn resource_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/parity/reaper/REAPER")
}

fn file_name(p: &std::path::Path) -> String {
    p.file_name().unwrap().to_string_lossy().into_owned()
}

#[test]
fn finds_templates_in_all_three_folders() {
    let names: Vec<_> = find_templates(&resource_dir())
        .iter()
        .map(|p| file_name(p))
        .collect();
    assert_eq!(
        names,
        [
            "Bus Glue.RfxChain",
            "Mix Template.RPP",
            "Lead Vocal.RTrackTemplate"
        ]
    );
}

#[test]
fn track_template_dependencies() {
    let all = scan_templates(&resource_dir());
    let t = all
        .iter()
        .find(|t| file_name(&t.template) == "Lead Vocal.RTrackTemplate")
        .unwrap();
    let hints: Vec<_> = t.plugins.iter().map(|p| p.identity_hint()).collect();
    assert_eq!(
        hints,
        [
            "jsfx:Acme/gate.jsfx",
            "jsfx:utility/volume",
            "vst2:Acme Comp x64.dll",
            "vst3:5653544163654D6163726F7665726200",
        ]
    );
    let verb = t.plugins.iter().find(|p| p.format == Vst3).unwrap();
    assert_eq!(
        (verb.name.as_str(), verb.vendor.as_deref()),
        ("Acme Verb", Some("Acme Audio"))
    );
    let gate = t
        .plugins
        .iter()
        .find(|p| p.key == "Acme/gate.jsfx")
        .unwrap();
    assert_eq!(gate.name, "gate");
}

#[test]
fn project_template_dependencies_are_unique() {
    let all = scan_templates(&resource_dir());
    let t = all
        .iter()
        .find(|t| file_name(&t.template) == "Mix Template.RPP")
        .unwrap();
    let got: Vec<_> = t
        .plugins
        .iter()
        .map(|p| (p.format, p.name.as_str()))
        .collect();
    assert_eq!(
        got,
        [
            (Au, "Acme Synth"),
            (Clap, "Acme Delay"),
            (Vst3, "Acme Piano"),
            (Vst3, "Acme Verb"),
        ]
    );
    let au = &t.plugins[0];
    assert_eq!(au.identity_hint(), "au:Acme Audio: Acme Synth");
    assert_eq!(t.plugins[1].identity_hint(), "clap:com.acmeaudio.delay");
    assert_eq!(
        t.plugins[2].identity_hint(),
        "vst3:5653544150494E4F4163656D6500AABB"
    );
}

#[test]
fn fx_chain_with_odd_quoting_and_non_plugin_blocks() {
    let all = scan_templates(&resource_dir());
    let t = all
        .iter()
        .find(|t| file_name(&t.template) == "Bus Glue.RfxChain")
        .unwrap();
    assert_eq!(t.plugins.len(), 1);
    let eq = &t.plugins[0];
    assert_eq!(eq.format, Vst2);
    assert_eq!(eq.name, "Acme EQ (Mono)");
    assert_eq!(eq.key, "Acme EQ Mono.dll");
}

#[test]
fn missing_resource_folder_is_empty() {
    assert!(scan_templates(&resource_dir().join("nope")).is_empty());
}
