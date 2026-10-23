//! First-scan budget (#42, CLAUDE.md rule 9): 500 plug-ins in under 60 s.
//!
//! Builds a synthetic tree of 500 plug-ins per OS layout from the fixtures
//! (100 each of five kinds, binaries padded to 1 MB so whole-file reads
//! would show), then times `scan_machine`. Ignored by default; CI runs it:
//! `cargo test -p patchpilot-scanner --test bench -- --ignored --nocapture`

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use patchpilot_core::{Arch, Os, PluginFormat};
use patchpilot_scanner::{ClapCache, PluginRoot, ScanInput, scan_machine};

const PER_KIND: usize = 100;
const BINARY_SIZE: u64 = 1 << 20;
const BUDGET: Duration = Duration::from_secs(60);

fn fixture(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/scanner")
        .join(rel)
}

/// (fixture, logical root, root format, file name pattern with `{}` for the name)
type Kind = (&'static str, &'static str, PluginFormat, &'static str);

const WINDOWS: [Kind; 5] = [
    (
        "vst3/pe-only/Acme Comp.vst3",
        "VST3_SYSTEM",
        PluginFormat::Vst3,
        "{}.vst3",
    ),
    (
        "vst3/moduleinfo-present/Acme Verb.vst3",
        "VST3_SYSTEM",
        PluginFormat::Vst3,
        "{} Info.vst3",
    ),
    (
        "vst2-aax/windows/Acme Comp x64.dll",
        "VST2_PROGRAM_FILES",
        PluginFormat::Vst2,
        "{}.dll",
    ),
    (
        "vst2-aax/windows/Acme Verb.aaxplugin",
        "AAX_SYSTEM",
        PluginFormat::Aax,
        "{}.aaxplugin",
    ),
    (
        "clap/windows/Acme Synth.clap",
        "CLAP_SYSTEM",
        PluginFormat::Clap,
        "{}.clap",
    ),
];

const MACOS: [Kind; 5] = [
    (
        "vst3/moduleinfo-present/Acme Verb.vst3",
        "VST3_SYSTEM",
        PluginFormat::Vst3,
        "{}.vst3",
    ),
    (
        "au/single-component/Acme Verb.component",
        "AU_SYSTEM",
        PluginFormat::Au,
        "{}.component",
    ),
    (
        "vst2-aax/macos/Acme Comp.vst",
        "VST2_SYSTEM",
        PluginFormat::Vst2,
        "{}.vst",
    ),
    (
        "vst2-aax/macos/Acme Verb.aaxplugin",
        "AAX_SYSTEM",
        PluginFormat::Aax,
        "{}.aaxplugin",
    ),
    (
        "clap/macos/Acme Synth.clap",
        "CLAP_SYSTEM",
        PluginFormat::Clap,
        "{}.clap",
    ),
];

#[test]
#[ignore = "benchmark; run with --ignored"]
fn first_scan_of_500_plugins_windows_layout() {
    bench("windows", Os::Windows, &WINDOWS);
}

#[test]
#[ignore = "benchmark; run with --ignored"]
fn first_scan_of_500_plugins_macos_layout() {
    bench("macos", Os::Macos, &MACOS);
}

fn bench(label: &str, os: Os, kinds: &[Kind]) {
    let dir = tempfile::tempdir().unwrap();
    let mut roots: Vec<PluginRoot> = Vec::new();
    for (k, (src, root, format, pattern)) in kinds.iter().enumerate() {
        let root_dir = dir.path().join(format!("{root}-{k}"));
        std::fs::create_dir_all(&root_dir).unwrap();
        for n in 0..PER_KIND {
            let name = format!("Bench Plugin {k}-{n:03}");
            copy(&fixture(src), &root_dir.join(pattern.replace("{}", &name)));
        }
        roots.push(PluginRoot::new(format!("{root}_{k}"), *format, &root_dir));
    }

    let start = Instant::now();
    let out = scan_machine(ScanInput {
        id: "bench".into(),
        name: "Bench".into(),
        os,
        arch: Arch::X86_64,
        os_version: None,
        roots,
        vendor_apps: Vec::new(),
        clap_cache: ClapCache::default(),
        now: 0,
    });
    let took = start.elapsed();

    let items = out.manifest.items.len();
    println!(
        "bench {label}: {items} plug-ins, {} products, {} warnings in {:.2} s; RSS {}",
        out.manifest.products.len(),
        out.warnings.len(),
        took.as_secs_f64(),
        rss().unwrap_or_else(|| "unknown".into()),
    );
    assert_eq!(items, kinds.len() * PER_KIND, "{:?}", out.warnings);
    assert!(
        took < BUDGET,
        "{label}: {took:?} is over the {BUDGET:?} budget"
    );
}

/// Copy a fixture file or bundle, padding binaries so a reader that loads
/// whole files would be slow.
fn copy(src: &Path, dst: &Path) {
    if src.is_dir() {
        std::fs::create_dir_all(dst).unwrap();
        for entry in std::fs::read_dir(src).unwrap() {
            let entry = entry.unwrap();
            copy(&entry.path(), &dst.join(entry.file_name()));
        }
        return;
    }
    std::fs::copy(src, dst).unwrap();
    let is_metadata = matches!(
        src.extension().and_then(|e| e.to_str()),
        Some("plist" | "json" | "ini" | "txt" | "md")
    );
    if !is_metadata {
        let f = std::fs::OpenOptions::new().write(true).open(dst).unwrap();
        if f.metadata().unwrap().len() < BINARY_SIZE {
            f.set_len(BINARY_SIZE).unwrap();
        }
    }
}

/// Resident memory of this process, from the OS's own tools (no unsafe).
fn rss() -> Option<String> {
    if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
        let line = status.lines().find(|l| l.starts_with("VmHWM:"))?;
        return Some(format!("peak {}", line["VmHWM:".len()..].trim()));
    }
    let pid = std::process::id().to_string();
    let out = if cfg!(windows) {
        std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
            .output()
            .ok()?
    } else {
        std::process::Command::new("ps")
            .args(["-o", "rss=", "-p", &pid])
            .output()
            .ok()?
    };
    let text = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    if cfg!(windows) {
        // "name","pid","session","#","12,345 K"
        text.rsplit("\",\"")
            .next()
            .map(|s| s.trim_matches('"').to_owned())
    } else {
        text.parse::<u64>()
            .ok()
            .map(|kb| format!("{} MB", kb / 1024))
    }
}
