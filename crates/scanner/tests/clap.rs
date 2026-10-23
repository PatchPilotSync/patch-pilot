use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use patchpilot_core::{Arch, Confidence, Os, PluginFormat};
use patchpilot_scanner::{ClapCache, MetaSource, PluginRoot, ScanInput, read_clap, scan_machine};

fn fixture(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/scanner/clap")
        .join(rel)
}

fn cache_text() -> String {
    std::fs::read_to_string(fixture("reaper/reaper-clap-win64.ini")).unwrap()
}

#[test]
fn reads_windows_clap_pe() {
    let m = read_clap(&fixture("windows/Acme Synth.clap"));
    assert_eq!(m.format, PluginFormat::Clap);
    assert_eq!(m.name, "Acme Synth");
    assert_eq!(m.vendor.as_deref(), Some("Acme Audio GmbH"));
    assert_eq!(m.version.as_deref(), Some("2.1.0"));
    assert_eq!(m.arch, Some(Arch::X86_64));
    assert_eq!(m.source, MetaSource::PeResource);
    assert_eq!(m.confidence, Confidence::Medium);
}

#[test]
fn reads_macos_clap_bundle() {
    let m = read_clap(&fixture("macos/Acme Synth.clap"));
    assert_eq!(m.name, "Acme Synth");
    assert_eq!(m.version.as_deref(), Some("2.1.0"));
    assert_eq!(m.bundle_id.as_deref(), Some("com.acmeaudio.synth"));
    assert_eq!(m.source, MetaSource::Plist);
}

#[test]
fn parses_reaper_cache() {
    let cache = ClapCache::parse(&cache_text(), None);
    let synth = cache
        .lookup(&fixture("windows/Acme Synth.clap"))
        .expect("cached");
    let ids: Vec<_> = synth.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(ids, ["com.acmeaudio.synth", "com.acmeaudio.synth-fx"]);
    assert_eq!(synth[1].name, "Acme Synth FX");
    assert_eq!(synth[1].vendor.as_deref(), Some("Acme Audio"));
    // Lookup is by file name, case-insensitive, on any OS.
    assert!(cache.lookup(&PathBuf::from("/x/acme synth.CLAP")).is_some());
    // Sections with no plug-in lines are dropped.
    assert!(cache.lookup(&PathBuf::from("Empty.clap")).is_none());
    assert!(cache.lookup(&fixture("windows/Mystery Box.clap")).is_none());
}

#[test]
fn stale_cache_is_ignored() {
    // A cache written before the plug-in file changed can't be trusted.
    let cache = ClapCache::parse(&cache_text(), Some(SystemTime::UNIX_EPOCH));
    assert!(cache.lookup(&fixture("windows/Acme Synth.clap")).is_none());
}

#[test]
fn loads_reaper_resource_dir() {
    let cache = ClapCache::load_reaper(&fixture("reaper"));
    assert!(!cache.is_empty());
    assert!(ClapCache::load_reaper(&fixture("missing")).is_empty());
}

fn scan(clap_cache: ClapCache) -> patchpilot_core::Manifest {
    scan_machine(ScanInput {
        id: "test".into(),
        name: "Test".into(),
        os: Os::Windows,
        arch: Arch::X86_64,
        os_version: None,
        roots: vec![PluginRoot::new(
            "CLAP_SYSTEM",
            PluginFormat::Clap,
            fixture("windows"),
        )],
        vendor_apps: Vec::new(),
        clap_cache,
        now: 1_800_000_000,
    })
    .manifest
}

#[test]
fn scan_uses_cache_ids_and_file_version() {
    let fresh = SystemTime::now() + Duration::from_secs(60);
    let m = scan(ClapCache::parse(&cache_text(), Some(fresh)));
    let mut ids: Vec<_> = m
        .items
        .iter()
        .map(|i| {
            (
                i.product_id.as_ref().unwrap().0.as_str(),
                i.version.as_deref(),
                i.confidence,
            )
        })
        .collect();
    ids.sort_by_key(|(id, ..)| id.to_string());
    assert_eq!(
        ids,
        [
            ("acme-audio/acme-synth", Some("2.1.0"), Confidence::High),
            ("acme-audio/acme-synth-fx", Some("2.1.0"), Confidence::High),
            // Not in the cache: PE metadata only.
            ("nobody-labs/mystery-box", Some("0.9"), Confidence::Medium),
        ]
    );
}

#[test]
fn scan_without_cache_reads_file_metadata() {
    let m = scan(ClapCache::default());
    let mut ids: Vec<_> = m
        .items
        .iter()
        .map(|i| i.product_id.as_ref().unwrap().0.clone())
        .collect();
    ids.sort();
    assert_eq!(ids, ["acme-audio/acme-synth", "nobody-labs/mystery-box"]);
}
