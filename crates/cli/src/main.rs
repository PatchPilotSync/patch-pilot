//! `patchpilot`: scan this machine into a manifest, or diff two manifests.
//!
//! Alpha-tester tool; the desktop app replaces it. Read-only: it never
//! changes plug-ins, DAW settings or any user file except `--out`. `--save`
//! and `history` use Patch Pilot's own database in the app-data folder.

mod anonymize;
mod golden;
mod report;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use patchpilot_core::{Arch, Manifest, Os, PluginFormat, TargetProfile};
use patchpilot_scanner::{PluginRoot, ScanInput, SystemDirs, default_roots};
use patchpilot_store::{SnapshotReason, Store};

#[derive(Parser)]
#[command(
    name = "patchpilot",
    version,
    about = "Patch Pilot alpha: scan and compare rigs"
)]
struct Cli {
    /// Database file. Defaults to $PATCHPILOT_DB, else the app-data folder
    /// (%LOCALAPPDATA%\\PatchPilot or ~/Library/Application Support/PatchPilot).
    #[arg(long, global = true, value_name = "PATH")]
    db: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Scan this machine's plug-ins and vendor apps into a manifest file
    /// and/or a snapshot in the local database.
    #[command(group = clap::ArgGroup::new("dest").required(true).multiple(true).args(["out", "save"]))]
    Scan {
        /// Where to write the manifest JSON.
        #[arg(long, short)]
        out: Option<PathBuf>,
        /// Save the scan as a snapshot in the local database (see `history`).
        #[arg(long)]
        save: bool,
        /// A name for this machine, e.g. "Studio PC". Defaults to the name
        /// saved by an earlier `--save` scan, else "This machine".
        #[arg(long)]
        name: Option<String>,
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
    /// List the saved scans (snapshots) of this machine, newest first.
    History {
        /// Another machine's ID instead of this one.
        #[arg(long)]
        machine: Option<String>,
    },
    /// Create, version and update the Golden Rig.
    Golden {
        #[command(subcommand)]
        command: golden::GoldenCommand,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let db = cli.db;
    let result = match cli.command {
        Command::Scan {
            out,
            save,
            name,
            folders,
            no_default_folders,
            no_vendor_apps,
            keep_paths,
        } => scan(
            out.as_deref(),
            save.then_some(db),
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
        Command::History { machine } => history(db, machine),
        Command::Golden { command } => golden::run(db, command),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("patchpilot: {e}");
            ExitCode::FAILURE
        }
    }
}

/// `save` is `Some(--db)` when the scan goes to the database.
fn scan(
    out: Option<&Path>,
    save: Option<Option<PathBuf>>,
    name: Option<String>,
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

    // A saved scan needs this machine's stable ID. A file-only scan reuses
    // it if it exists but never creates app-data files.
    let mut store = None;
    let id = match &save {
        Some(db) => {
            let path = db_path(db.clone())?;
            let dir = app_dir(&path);
            let id = patchpilot_store::load_or_create_machine_id(dir).map_err(|e| e.to_string())?;
            store = Some((open_store(&path)?, path));
            id
        }
        None => match db_path(None) {
            Ok(path) => patchpilot_store::read_machine_id(app_dir(&path))
                .ok()
                .flatten()
                .unwrap_or_else(random_id),
            Err(_) => random_id(),
        },
    };
    let saved_name = store
        .as_ref()
        .and_then(|(s, _)| s.machine(&id).ok())
        .map(|m| m.name);
    let name = name
        .or(saved_name)
        .unwrap_or_else(|| "This machine".to_owned());

    let output = patchpilot_scanner::scan_machine(ScanInput {
        id,
        name,
        os,
        arch: current_arch(),
        os_version: None,
        roots,
        vendor_apps,
        clap_cache: reaper_clap_cache(os, &dirs),
        now: now(),
    });
    let manifest = output.manifest;
    for w in &output.warnings {
        eprintln!("warning: {w}");
    }
    let plugins = manifest
        .items
        .iter()
        .filter(|i| i.kind == patchpilot_core::ItemKind::Plugin)
        .count();
    let counts = format!(
        "{plugins} plug-ins, {} products, {} warnings",
        manifest.products.len(),
        output.warnings.len()
    );

    // The local database keeps real paths; it never leaves this machine.
    if let Some((mut store, path)) = store {
        let first = store
            .snapshots(&manifest.machine.id)
            .map_err(|e| e.to_string())?
            .is_empty();
        let reason = if first {
            SnapshotReason::FirstScan
        } else {
            SnapshotReason::Scan
        };
        let info = store
            .save_manifest(&manifest, reason, now())
            .map_err(|e| format!("{}: {e}", path.display()))?;
        if info.unchanged {
            println!("No changes since snapshot #{} ({counts})", info.id);
        } else {
            println!(
                "Saved snapshot #{} of {} ({counts})",
                info.id, manifest.machine.name
            );
        }
    }

    if let Some(out) = out {
        let mut manifest = manifest;
        if !keep_paths {
            anonymize_for_sharing(&mut manifest);
        }
        std::fs::write(out, manifest.to_json_pretty() + "\n")
            .map_err(|e| format!("{}: {e}", out.display()))?;
        println!("Wrote {} ({counts})", out.display());
    }
    Ok(())
}

fn history(db: Option<PathBuf>, machine: Option<String>) -> Result<(), String> {
    let path = db_path(db)?;
    let id = this_or(&path, machine)?;
    let store = open_existing_store(&path)?;
    let name = store.machine(&id).map_err(|e| e.to_string())?.name;
    let snapshots = store.snapshots(&id).map_err(|e| e.to_string())?;
    print!("{}", report::history(&name, &snapshots));
    Ok(())
}

/// `machine`, else this machine's ID from app-data.
fn this_or(db: &Path, machine: Option<String>) -> Result<String, String> {
    match machine {
        Some(id) => Ok(id),
        None => patchpilot_store::read_machine_id(app_dir(db))
            .map_err(|e| e.to_string())?
            .ok_or_else(|| {
                "no saved scans on this machine yet; run `patchpilot scan --save`".to_owned()
            }),
    }
}

/// Open the database for reading commands; never creates one.
fn open_existing_store(path: &Path) -> Result<Store, String> {
    if !path.exists() {
        return Err(format!(
            "{}: no database yet; run `patchpilot scan --save`",
            path.display()
        ));
    }
    open_store(path)
}

/// Strip home folder and username from paths before a manifest is written.
fn anonymize_for_sharing(m: &mut patchpilot_core::Manifest) {
    let home = home_dir();
    let user = std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .ok();
    anonymize::anonymize(m, home.as_deref(), user.as_deref());
}

/// `--db`, else `$PATCHPILOT_DB`, else the app-data default.
fn db_path(flag: Option<PathBuf>) -> Result<PathBuf, String> {
    if let Some(p) = flag {
        return Ok(p);
    }
    if let Some(p) = std::env::var_os("PATCHPILOT_DB").filter(|p| !p.is_empty()) {
        return Ok(PathBuf::from(p));
    }
    let os = Os::current().ok_or("no default database on this OS; pass --db")?;
    let local = std::env::var("LOCALAPPDATA").unwrap_or_default();
    patchpilot_store::default_db_path(os, &local, &home_dir().unwrap_or_default())
        .ok_or_else(|| "can't find the app-data folder; pass --db".to_owned())
}

/// The folder holding the database, where `machine-id` lives too.
fn app_dir(db: &Path) -> &Path {
    db.parent().unwrap_or(Path::new("."))
}

fn open_store(path: &Path) -> Result<Store, String> {
    Store::open(path).map_err(|e| format!("{}: {e}", path.display()))
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

/// A per-scan machine ID for file-only scans before any `--save`.
fn random_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let seed = format!("{nanos}-{}-{:p}", std::process::id(), &nanos);
    blake3::hash(seed.as_bytes()).to_hex()[..16].to_owned()
}
