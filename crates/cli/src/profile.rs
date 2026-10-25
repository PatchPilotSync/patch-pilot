//! `patchpilot profile ...`: stored target profiles (#36).

use std::path::PathBuf;

use clap::Subcommand;
use patchpilot_core::{Matcher, RuleAction, TargetProfile};

use crate::{db_path, open_existing_store, resolve_machine};

#[derive(Subcommand)]
pub enum ProfileCommand {
    /// List target profiles and the machines that use them.
    List,
    /// Show one profile's rules.
    Show { id: String },
    /// Assign a profile to a machine (ID or name). `none` clears it.
    Set { machine: String, profile: String },
    /// Add or replace a profile from a JSON file (built-ins can't be replaced).
    Import { file: PathBuf },
}

pub fn run(db: Option<PathBuf>, cmd: ProfileCommand) -> Result<(), String> {
    let path = db_path(db)?;
    let mut store = open_existing_store(&path)?;
    let err = |e: patchpilot_store::StoreError| e.to_string();
    match cmd {
        ProfileCommand::List => {
            for p in store.target_profiles().map_err(err)? {
                let tag = if p.builtin { "  (built-in)" } else { "" };
                println!("{:<16} {}{tag}", p.profile.id, p.profile.name);
                let names: Vec<_> = p
                    .machines
                    .iter()
                    .map(|id| store.machine(id).map_or_else(|_| id.clone(), |m| m.name))
                    .collect();
                if !names.is_empty() {
                    println!("{:<16} used by {}", "", names.join(", "));
                }
            }
        }
        ProfileCommand::Show { id } => {
            let p = store
                .target_profile(&id)
                .map_err(err)?
                .ok_or_else(|| format!("unknown target profile {id:?}"))?;
            print!("{}", describe(&p));
        }
        ProfileCommand::Set { machine, profile } => {
            let m = resolve_machine(&store, &machine)?;
            let profile = (!profile.eq_ignore_ascii_case("none")).then_some(profile);
            store
                .set_machine_profile(&m.id, profile.as_deref())
                .map_err(err)?;
            match profile {
                Some(p) => println!("{} now uses profile {p}", m.name),
                None => println!("{} now uses the whole Primary Rig", m.name),
            }
        }
        ProfileCommand::Import { file } => {
            let text =
                std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
            let p: TargetProfile =
                serde_json::from_str(&text).map_err(|e| format!("{}: {e}", file.display()))?;
            store.save_target_profile(&p).map_err(err)?;
            println!("Saved profile {} ({} rules)", p.id, p.rules.len());
        }
    }
    Ok(())
}

/// `Travel (travel)` then one line per rule, in the order they apply.
pub fn describe(p: &TargetProfile) -> String {
    let mut out = format!("{} ({})\n", p.name, p.id);
    if p.rules.is_empty() {
        out.push_str("  everything in the Primary Rig\n");
    }
    for (i, r) in p.rules.iter().enumerate() {
        let action = match r.action {
            RuleAction::Include => "include",
            RuleAction::Exclude => "exclude",
        };
        let what = match &r.matcher {
            Matcher::Vendor(v) => format!("vendor {v}"),
            Matcher::Format(f) => format!("format {}", json_name(f).to_uppercase()),
            Matcher::Kind(k) => format!("kind {}", json_name(k)),
            Matcher::Product(id) => format!("product {id}"),
            Matcher::SizeOver(n) => format!("items over {}", size(*n)),
            Matcher::PathGlob(g) => format!("paths {g}"),
        };
        out.push_str(&format!("  {}. {action} {what}\n", i + 1));
    }
    if p.rules.len() > 1 {
        out.push_str("  (the last matching rule wins)\n");
    }
    out
}

fn json_name<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_value(v)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn size(bytes: u64) -> String {
    const GB: u64 = 1_000_000_000;
    if bytes >= GB && bytes.is_multiple_of(GB) {
        format!("{} GB", bytes / GB)
    } else {
        format!("{bytes} bytes")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describes_travel() {
        let p: TargetProfile = serde_json::from_str(
            r#"{"id":"travel","name":"Travel","rules":[
                {"action":"exclude","match":{"format":"aax"}},
                {"action":"exclude","match":{"size_over":20000000000}}]}"#,
        )
        .unwrap();
        assert_eq!(
            describe(&p),
            "Travel (travel)\n  1. exclude format AAX\n  2. exclude items over 20 GB\n  (the last matching rule wins)\n"
        );
    }
}
