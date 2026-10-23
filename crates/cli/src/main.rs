//! `patchpilot`: scan this machine into a manifest, or diff two manifests.
//!
//! Alpha-tester tool; the desktop app replaces it. Read-only: it never
//! changes plug-ins, DAW settings or any user file except `--out`. `--save`
//! and `history` use Patch Pilot's own database in the app-data folder.

mod exchange;
mod golden;
mod profile;
mod report;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use patchpilot_core::{Manifest, TargetProfile};
use patchpilot_local::ScanOptions;
pub(crate) use patchpilot_local::{anonymize_for_sharing, app_dir, now};
use patchpilot_store::Store;

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

#[derive(clap::Args)]
#[command(group = clap::ArgGroup::new("dest").required(true).multiple(true).args(["out", "save"]))]
struct ScanArgs {
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
    /// Reaper resource folder for templates and the CLAP cache. Default: the
    /// standard install location (skipped with --no-default-folders).
    #[arg(long, value_name = "DIR")]
    reaper_dir: Option<PathBuf>,
}

/// What to compare: two manifest files, or a saved machine from the database.
#[derive(clap::Args)]
struct CompareArgs {
    /// Golden manifest JSON.
    #[arg(requires = "machine_file")]
    golden: Option<PathBuf>,
    /// Machine manifest JSON.
    #[arg(id = "machine_file", value_name = "MACHINE")]
    machine: Option<PathBuf>,
    /// Saved machine (ID or name) when comparing from the database. Default:
    /// this machine.
    #[arg(long = "machine", value_name = "MACHINE", conflicts_with = "golden")]
    saved_machine: Option<String>,
    /// Target profile JSON (e.g. Travel: no AAX). Default: the machine's
    /// stored profile (see `profile set`).
    #[arg(long)]
    profile: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Command {
    /// Scan this machine's plug-ins and vendor apps into a manifest file
    /// and/or a snapshot in the local database.
    Scan(ScanArgs),
    /// Compare a machine against the Golden Rig.
    ///
    /// With no files: a machine's latest saved scan against the latest Golden
    /// version, using the machine's stored target profile.
    Diff {
        #[command(flatten)]
        cmp: CompareArgs,
        /// Also list products that are in parity.
        #[arg(long)]
        all: bool,
    },
    /// Plan the steps that bring a machine up to speed with the Golden Rig.
    ///
    /// A read-only checklist: nothing is installed or copied. Takes the same
    /// inputs as `diff`.
    Plan {
        #[command(flatten)]
        cmp: CompareArgs,
    },
    /// List the saved scans (snapshots) of this machine, newest first.
    History {
        /// Another machine's ID instead of this one.
        #[arg(long)]
        machine: Option<String>,
    },
    /// List, show, import and assign target profiles.
    Profile {
        #[command(subcommand)]
        command: profile::ProfileCommand,
    },
    /// Create, version and update the Golden Rig.
    Golden {
        #[command(subcommand)]
        command: golden::GoldenCommand,
    },
    /// Write saved scans, every Golden version and your profiles to one file
    /// for another machine's `patchpilot import`.
    ///
    /// Defaults to this machine's latest scan. Your username is removed from
    /// paths unless `--keep-paths`.
    Export {
        #[arg(long, short)]
        out: PathBuf,
        /// Machine ID or name instead of this machine. Repeatable.
        #[arg(
            long = "machine",
            value_name = "MACHINE",
            conflicts_with = "all_machines"
        )]
        machines: Vec<String>,
        /// Every machine in the database.
        #[arg(long)]
        all_machines: bool,
        #[arg(long)]
        keep_paths: bool,
    },
    /// Add what an `export` file holds to this machine's database.
    ///
    /// Never overwrites: scans become new snapshots, Golden versions are added
    /// only if this machine's Golden Rig is behind, and your own profiles win.
    Import { file: PathBuf },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let db = cli.db;
    let result = match cli.command {
        Command::Scan(args) => scan(args, db),
        Command::Diff { cmp, all } => diff(db, cmp, all),
        Command::Plan { cmp } => plan(db, cmp),
        Command::History { machine } => history(db, machine),
        Command::Golden { command } => golden::run(db, command),
        Command::Profile { command } => profile::run(db, command),
        Command::Export {
            out,
            machines,
            all_machines,
            keep_paths,
        } => exchange::export(db, &out, machines, all_machines, keep_paths),
        Command::Import { file } => exchange::import(db, &file),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("patchpilot: {e}");
            ExitCode::FAILURE
        }
    }
}

fn scan(args: ScanArgs, db: Option<PathBuf>) -> Result<(), String> {
    let ScanArgs {
        out,
        save,
        name,
        folders,
        no_default_folders,
        no_vendor_apps,
        keep_paths,
        reaper_dir,
    } = args;
    let out = out.as_deref();
    // `Some(--db)` when the scan goes to the database.
    let save = save.then_some(db);
    let extra_roots = folders
        .iter()
        .enumerate()
        .map(|(i, spec)| {
            patchpilot_local::extra_root(i + 1, spec).map_err(|e| format!("--folder {e}"))
        })
        .collect::<Result<Vec<_>, _>>()?;

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
                .unwrap_or_else(patchpilot_local::random_id),
            Err(_) => patchpilot_local::random_id(),
        },
    };
    let saved_name = store
        .as_ref()
        .and_then(|(s, _)| s.machine(&id).ok())
        .map(|m| m.name);
    let name = name
        .or(saved_name)
        .unwrap_or_else(|| "This machine".to_owned());

    let scan = patchpilot_local::scan_this_machine(
        ScanOptions {
            id,
            name,
            default_folders: !no_default_folders,
            extra_roots,
            vendor_apps: !no_vendor_apps,
            reaper_dir,
        },
        |_| {},
    )?;
    for w in &scan.warnings {
        eprintln!("warning: {w}");
    }
    let counts = format!(
        "{} plug-ins, {} templates, {} products, {} warnings",
        scan.plugins(),
        scan.templates,
        scan.manifest.products.len(),
        scan.warnings.len()
    );
    let manifest = scan.manifest;

    // The local database keeps real paths; it never leaves this machine.
    if let Some((mut store, path)) = store {
        let info = patchpilot_local::save_scan(&mut store, &manifest)
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

/// `--db`, else `$PATCHPILOT_DB`, else the app-data default.
fn db_path(flag: Option<PathBuf>) -> Result<PathBuf, String> {
    match flag {
        Some(p) => Ok(p),
        None => patchpilot_local::default_db_path().map_err(|e| format!("{e}; pass --db")),
    }
}

fn open_store(path: &Path) -> Result<Store, String> {
    Store::open(path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Load and align the two sides of a comparison and the profile to use.
fn load_comparison(
    db: Option<PathBuf>,
    cmp: CompareArgs,
) -> Result<(Manifest, Manifest, TargetProfile), String> {
    let read = |p: &Path| std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()));
    let (mut golden_m, mut machine_m, stored_profile) = match cmp.golden.zip(cmp.machine) {
        Some((golden, machine)) => {
            let golden_m = Manifest::from_json(&read(&golden)?)
                .map_err(|e| format!("{}: {e}", golden.display()))?;
            let machine_m = Manifest::from_json(&read(&machine)?)
                .map_err(|e| format!("{}: {e}", machine.display()))?;
            // Use the machine's stored profile if this machine is known here;
            // never create a database just to look.
            let stored = db_path(db)
                .ok()
                .filter(|p| p.exists())
                .and_then(|p| open_store(&p).ok())
                .and_then(|s| s.machine_profile(&machine_m.machine.id).ok().flatten());
            (golden_m, machine_m, stored)
        }
        None => {
            let path = db_path(db)?;
            let store = open_existing_store(&path)?;
            let machine = match cmp.saved_machine {
                Some(q) => resolve_machine(&store, &q)?,
                None => {
                    let id = this_or(&path, None)?;
                    store.machine(&id).map_err(|e| e.to_string())?
                }
            };
            let golden = store
                .golden(None)
                .map_err(|e| e.to_string())?
                .ok_or("no Golden Rig yet; run `patchpilot golden init`")?;
            let scan = store
                .latest_manifest(&machine.id)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("no saved scan of {}", machine.name))?;
            let stored = store
                .machine_profile(&machine.id)
                .map_err(|e| e.to_string())?;
            (golden.manifest, scan, stored)
        }
    };
    let profile: TargetProfile = match cmp.profile {
        Some(p) => serde_json::from_str(&read(&p)?).map_err(|e| format!("{}: {e}", p.display()))?,
        None => stored_profile.unwrap_or_default(),
    };
    patchpilot_parity::align_products(&mut golden_m, &mut machine_m);
    Ok((golden_m, machine_m, profile))
}

fn diff(db: Option<PathBuf>, cmp: CompareArgs, all: bool) -> Result<(), String> {
    let (golden_m, machine_m, profile) = load_comparison(db, cmp)?;
    // Plug-ins any template on either side uses weigh more (#37).
    let needed = patchpilot_parity::required_products(&[&golden_m, &machine_m]);
    let score = patchpilot_parity::score_machine(
        &golden_m,
        &profile,
        &machine_m,
        &needed,
        &patchpilot_parity::ScoreWeights::default(),
    );
    let readiness = patchpilot_parity::template_readiness(&machine_m);
    print!(
        "{}",
        report::render(&golden_m, &machine_m, &profile, &score, &readiness, all)
    );
    Ok(())
}

fn plan(db: Option<PathBuf>, cmp: CompareArgs) -> Result<(), String> {
    let (golden_m, machine_m, profile) = load_comparison(db, cmp)?;
    let plan = patchpilot_parity::up_to_speed(&golden_m, &profile, &machine_m);
    print!("{}", report::plan(&golden_m, &machine_m, &profile, &plan));
    Ok(())
}

/// A saved machine by ID, or by name (case-insensitive) if unambiguous.
fn resolve_machine(store: &Store, query: &str) -> Result<patchpilot_core::MachineInfo, String> {
    let machines = store.machines().map_err(|e| e.to_string())?;
    if let Some(m) = machines.iter().find(|m| m.id == query) {
        return Ok(m.clone());
    }
    let mut named = machines
        .into_iter()
        .filter(|m| m.name.eq_ignore_ascii_case(query.trim()));
    match (named.next(), named.next()) {
        (Some(m), None) => Ok(m),
        (Some(_), Some(_)) => Err(format!(
            "more than one machine is named {query:?}; use its ID"
        )),
        (None, _) => Err(format!("no saved machine {query:?}")),
    }
}
