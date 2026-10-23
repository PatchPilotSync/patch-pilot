//! `patchpilot golden ...`: the versioned Golden Profile (#35).

use std::path::{Path, PathBuf};

use clap::Subcommand;
use patchpilot_core::{GoldenVersionInfo, Manifest};
use patchpilot_store::{Store, StoreError};

use crate::{anonymize_for_sharing, db_path, now, open_existing_store, report, this_or};

#[derive(Subcommand)]
pub enum GoldenCommand {
    /// Make a machine's latest saved scan Golden v1 ("Make this your Golden Rig").
    Init {
        /// Machine ID (default: this machine).
        #[arg(long)]
        machine: Option<String>,
        #[arg(long)]
        note: Option<String>,
    },
    /// Replace the Golden Rig with a machine's latest saved scan, as a new version.
    Promote {
        /// Machine ID (default: this machine).
        #[arg(long)]
        machine: Option<String>,
        #[arg(long)]
        note: Option<String>,
    },
    /// Take selected products from a machine into a new Golden version.
    Propose {
        /// Machine ID (default: this machine).
        #[arg(long)]
        from: Option<String>,
        /// Product ID or name. Repeatable.
        #[arg(long = "product", required = true, value_name = "PRODUCT")]
        products: Vec<String>,
        #[arg(long)]
        note: Option<String>,
    },
    /// Show a Golden version (default: the latest).
    Show {
        #[arg(long)]
        version: Option<u32>,
        /// Also write that version's manifest JSON here (usable with `diff`).
        #[arg(long, short)]
        out: Option<PathBuf>,
        /// Keep real paths in `--out`, including your username.
        #[arg(long)]
        keep_paths: bool,
    },
    /// List every Golden version, newest first.
    History,
}

pub fn run(db: Option<PathBuf>, cmd: GoldenCommand) -> Result<(), String> {
    let path = db_path(db)?;
    let mut store = open_existing_store(&path)?;
    match cmd {
        GoldenCommand::Init { machine, note } => {
            if let Some(g) = store.golden(None).map_err(err)? {
                return Err(format!(
                    "the Golden Rig already exists (v{}); use `golden promote` or `golden propose`",
                    g.info.version
                ));
            }
            let (id, scan) = latest_scan(&store, &path, machine)?;
            let v = save(&mut store, &scan, &id, note)?;
            println!(
                "Created Golden Rig v{} from {}",
                v.version, scan.machine.name
            );
        }
        GoldenCommand::Promote { machine, note } => {
            require_golden(&store)?;
            let (id, scan) = latest_scan(&store, &path, machine)?;
            let v = save(&mut store, &scan, &id, note)?;
            println!(
                "Golden Rig v{} is now {}'s latest scan (was v{})",
                v.version,
                scan.machine.name,
                v.parent.unwrap_or_default()
            );
        }
        GoldenCommand::Propose {
            from,
            products,
            note,
        } => {
            let golden = require_golden(&store)?;
            let (id, scan) = latest_scan(&store, &path, from)?;
            let next = patchpilot_parity::propose_to_golden(&golden, &scan, &products)
                .map_err(|e| e.to_string())?;
            let v = save(&mut store, &next, &id, note)?;
            println!(
                "Golden Rig v{} takes {} product(s) from {} (was v{})",
                v.version,
                products.len(),
                scan.machine.name,
                v.parent.unwrap_or_default()
            );
        }
        GoldenCommand::Show {
            version,
            out,
            keep_paths,
        } => {
            let g = match store.golden(version).map_err(err)? {
                Some(g) => g,
                None if version.is_some() => {
                    return Err(format!("there is no Golden Rig v{}", version.unwrap_or(0)));
                }
                None => return Err(NO_GOLDEN.to_owned()),
            };
            let source = source_name(&store, &g.info);
            print!(
                "{}",
                report::golden(&g.info, source.as_deref(), &g.manifest)
            );
            if let Some(out) = out {
                let mut m = g.manifest;
                if !keep_paths {
                    anonymize_for_sharing(&mut m);
                }
                write_json(&out, &m)?;
                println!("Wrote {}", out.display());
            }
        }
        GoldenCommand::History => {
            let versions = store.golden_history().map_err(err)?;
            if versions.is_empty() {
                return Err(NO_GOLDEN.to_owned());
            }
            let rows: Vec<_> = versions
                .iter()
                .map(|v| (v, source_name(&store, v)))
                .collect();
            print!("{}", report::golden_history(&rows));
        }
    }
    Ok(())
}

const NO_GOLDEN: &str = "no Golden Rig yet; run `patchpilot golden init`";

fn err(e: StoreError) -> String {
    e.to_string()
}

fn require_golden(store: &Store) -> Result<Manifest, String> {
    store
        .golden(None)
        .map_err(err)?
        .map(|g| g.manifest)
        .ok_or_else(|| NO_GOLDEN.to_owned())
}

/// A machine's ID and its latest saved scan.
fn latest_scan(
    store: &Store,
    db: &Path,
    machine: Option<String>,
) -> Result<(String, Manifest), String> {
    let id = this_or(db, machine)?;
    let scan = store
        .latest_manifest(&id)
        .map_err(err)?
        .ok_or_else(|| format!("no saved scan of machine {id}; run `patchpilot scan --save`"))?;
    Ok((id, scan))
}

fn save(
    store: &mut Store,
    manifest: &Manifest,
    source: &str,
    note: Option<String>,
) -> Result<GoldenVersionInfo, String> {
    store
        .save_golden(manifest, Some(source), note.as_deref(), now())
        .map_err(err)
}

fn source_name(store: &Store, v: &GoldenVersionInfo) -> Option<String> {
    let id = v.source_machine_id.as_deref()?;
    Some(store.machine(id).map_or_else(|_| id.to_owned(), |m| m.name))
}

fn write_json(out: &Path, m: &Manifest) -> Result<(), String> {
    std::fs::write(out, m.to_json_pretty() + "\n").map_err(|e| format!("{}: {e}", out.display()))
}
