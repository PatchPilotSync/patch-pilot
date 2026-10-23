//! `patchpilot`: scan this machine into a manifest, or diff two manifests.
//!
//! Alpha-tester tool; the desktop app replaces it. Read-only: it never
//! changes plug-ins, DAW settings or any user file except `--out`.

mod anonymize;
mod report;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use patchpilot_core::{Arch, Manifest, Os, PluginFormat, TargetProfile};
use patchpilot_scanner::{PluginRoot, ScanInput, SystemDirs, default_roots};

#[derive(Parser)]
#[command(
    name = "patchpilot",
    version,
    about = "Patch Pilot alpha: scan and compare rigs"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Scan this machine's plug-ins and vendor apps into a manifest file.
    Scan {
        /// Where to write the manifest JSON.
        #[arg(long, short)]
        out: PathBuf,
        /// A name for this machine, e.g. "Studio PC".
        #[arg(long, default_value = "This machine")]
        name: String,
        /// Extra plug-in folder as FORMAT=PATH (vst2, vst3, au, aax, clap). Repeatable.
        #[arg(long = "folder", value_name = "FORMAT=PATH")]
        folders: Vec<String>,
        /// Skip the standard plug-in folders (only scan --folder ones).
        #[arg(long)]
        no_default_folders: bool,
        /// Skip vendor app and iLok detection.
        #[arg(long)]
        no_vendor_apps: bool,
        /// Keep real paths, including your username. Off by default so the
        /// file is safe to share.
        #[arg(long)]
        keep_paths: bool,
    },
    /// Compare a machine's manifest against a Golden manifest.
    Diff {
        golden: PathBuf,
        machine: PathBuf,
        /// Target profile JSON (e.g. Travel: no AAX).
        #[arg(long)]
        profile: Option<PathBuf>,
        /// Also list products that are in parity.
        #[arg(long)]
        all: bool,
    },
}

fn main() -> ExitCode {
    let result = match Cli::parse().command {
        Command::Scan {
            out,
            name,
            folders,
            no_default_folders,
            no_vendor_apps,
            keep_paths,
        } => scan(
            &out,
            name,
            &folders,
            no_default_folders,
            no_vendor_apps,
            keep_paths,
        ),
        Command::Diff {
            golden,
            machine,
            profile,
            all,
        } => diff(&golden, &machine, profile.as_deref(), all),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("patchpilot: {e}");
            ExitCode::FAILURE
        }
    }
}

fn scan(
    out: &Path,
    name: String,
    folders: &[String],
    no_default_folders: bool,
    no_vendor_apps: bool,
    keep_paths: bool,
) -> Result<(), String> {
    let os = Os::current().ok_or("scanning is supported on Windows and macOS only")?;
    let dirs = SystemDirs::from_env().ok_or("unsupported OS")?;

    let mut roots = if no_default_folders {
        Vec::new()
    } else {
        default_roots(&dirs)
    };
    for (i, spec) in folders.iter().enumerate() {
        let (format, path) = spec
            .split_once('=')
            .ok_or_else(|| format!("--folder {spec:?}: expected FORMAT=PATH"))?;
        let format: PluginFormat =
            serde_json::from_value(serde_json::Value::String(format.to_lowercase()))
                .map_err(|_| format!("--folder {spec:?}: unknown format {format:?}"))?;
        roots.push(PluginRoot::new(format!("EXTRA_{}", i + 1), format, path));
    }

    let vendor_apps = if no_vendor_apps {
        Vec::new()
    } else {
        detect_vendor_apps(os, &dirs)
    };

    let output = patchpilot_scanner::scan_machine(ScanInput {
        id: random_id(),
        name,
        os,
        arch: current_arch(),
        os_version: None,
        roots,
        vendor_apps,
        clap_cache: reaper_clap_cache(os, &dirs),
        now: now(),
    });
    let mut manifest = output.manifest;
    if !keep_paths {
        let home = home_dir();
        let user = std::env::var("USERNAME")
            .or_else(|_| std::env::var("USER"))
            .ok();
        anonymize::anonymize(&mut manifest, home.as_deref(), user.as_deref());
    }

    std::fs::write(out, manifest.to_json_pretty() + "\n")
        .map_err(|e| format!("{}: {e}", out.display()))?;
    for w in &output.warnings {
        eprintln!("warning: {w}");
    }
    let plugins = manifest
        .items
        .iter()
        .filter(|i| i.kind == patchpilot_core::ItemKind::Plugin)
        .count();
    println!(
        "Wrote {} ({plugins} plug-ins, {} products, {} warnings)",
        out.display(),
        manifest.products.len(),
        output.warnings.len()
    );
    Ok(())
}

fn diff(golden: &Path, machine: &Path, profile: Option<&Path>, all: bool) -> Result<(), String> {
    let read = |p: &Path| std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()));
    let mut golden_m =
        Manifest::from_json(&read(golden)?).map_err(|e| format!("{}: {e}", golden.display()))?;
    let mut machine_m =
        Manifest::from_json(&read(machine)?).map_err(|e| format!("{}: {e}", machine.display()))?;
    let profile: TargetProfile = match profile {
        Some(p) => serde_json::from_str(&read(p)?).map_err(|e| format!("{}: {e}", p.display()))?,
        None => TargetProfile::default(),
    };
    patchpilot_parity::align_products(&mut golden_m, &mut machine_m);
    let score = patchpilot_parity::score_machine(
        &golden_m,
        &profile,
        &machine_m,
        &BTreeSet::new(),
        &patchpilot_parity::ScoreWeights::default(),
    );
    print!(
        "{}",
        report::render(&golden_m, &machine_m, &profile, &score, all)
    );
    Ok(())
}

#[cfg(windows)]
fn detect_vendor_apps(_os: Os, _dirs: &SystemDirs) -> Vec<patchpilot_scanner::VendorAppStatus> {
    patchpilot_scanner::detect_windows(&patchpilot_scanner::read_uninstall_entries())
}

#[cfg(not(windows))]
fn detect_vendor_apps(os: Os, dirs: &SystemDirs) -> Vec<patchpilot_scanner::VendorAppStatus> {
    match os {
        Os::Macos => {
            let mut roots = vec![PathBuf::from("/")];
            if !dirs.home.is_empty() {
                roots.push(PathBuf::from(&dirs.home));
            }
            patchpilot_scanner::detect_macos(&roots)
        }
        Os::Windows => Vec::new(),
    }
}

/// Reaper's CLAP cache gives real CLAP IDs without loading plug-ins (#8).
fn reaper_clap_cache(os: Os, dirs: &SystemDirs) -> patchpilot_scanner::ClapCache {
    let appdata = std::env::var("APPDATA").unwrap_or_default();
    patchpilot_parity::reaper::reaper_resource_dir(os, &appdata, &dirs.home)
        .map(|dir| patchpilot_scanner::ClapCache::load_reaper(&dir))
        .unwrap_or_default()
}

fn home_dir() -> Option<String> {
    std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .ok()
        .filter(|h| !h.is_empty())
}

fn current_arch() -> Arch {
    if cfg!(target_arch = "aarch64") {
        Arch::Aarch64
    } else {
        Arch::X86_64
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// A random-enough per-scan machine ID. Not derived from hardware.
fn random_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let seed = format!("{nanos}-{}-{:p}", std::process::id(), &nanos);
    blake3::hash(seed.as_bytes()).to_hex()[..16].to_owned()
}
