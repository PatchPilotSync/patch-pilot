//! `patchpilot export` / `import`: move scans, the Primary Rig and profiles
//! between machines in one file, without a cloud (#38).

use std::path::{Path, PathBuf};

use patchpilot_core::Bundle;
use patchpilot_store::{GoldenImport, ImportReport};

use crate::{
    anonymize_for_sharing, app_dir, db_path, now, open_existing_store, open_store, resolve_machine,
};

pub fn export(
    db: Option<PathBuf>,
    out: &Path,
    machines: Vec<String>,
    all_machines: bool,
    keep_paths: bool,
) -> Result<(), String> {
    let path = db_path(db)?;
    let store = open_existing_store(&path)?;
    let this = patchpilot_store::read_machine_id(app_dir(&path)).map_err(|e| e.to_string())?;

    let ids: Vec<String> = if all_machines {
        let all = store.machines().map_err(|e| e.to_string())?;
        all.into_iter().map(|m| m.id).collect()
    } else if machines.is_empty() {
        // This machine's latest scan, if it has one.
        match &this {
            Some(id)
                if store
                    .latest_manifest(id)
                    .map_err(|e| e.to_string())?
                    .is_some() =>
            {
                vec![id.clone()]
            }
            _ => Vec::new(),
        }
    } else {
        machines
            .iter()
            .map(|q| resolve_machine(&store, q).map(|m| m.id))
            .collect::<Result<_, _>>()?
    };
    let from = match &this {
        Some(id) => store.machine(id).ok().map(|m| m.name),
        None => None,
    };

    let mut bundle = store
        .export_bundle(&ids, from.as_deref(), now())
        .map_err(|e| e.to_string())?;
    if bundle.machines.is_empty() && bundle.golden.is_empty() && bundle.profiles.is_empty() {
        return Err("nothing to export; run `patchpilot scan --save` first".to_owned());
    }
    if !keep_paths {
        let manifests = bundle
            .machines
            .iter_mut()
            .map(|m| &mut m.manifest)
            .chain(bundle.golden.iter_mut().map(|g| &mut g.manifest));
        for m in manifests {
            anonymize_for_sharing(m);
        }
    }
    std::fs::write(out, bundle.to_json_pretty() + "\n")
        .map_err(|e| format!("{}: {e}", out.display()))?;
    println!(
        "Wrote {} ({} machine(s), {} Primary Rig version(s), {} profile(s))",
        out.display(),
        bundle.machines.len(),
        bundle.golden.len(),
        bundle.profiles.len()
    );
    Ok(())
}

pub fn import(db: Option<PathBuf>, file: &Path) -> Result<(), String> {
    let text = std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let bundle = Bundle::from_json(&text).map_err(|e| format!("{}: {e}", file.display()))?;
    let path = db_path(db)?;
    let mut store = open_store(&path)?;
    let this = patchpilot_store::read_machine_id(app_dir(&path)).map_err(|e| e.to_string())?;
    let report = store
        .import_bundle(&bundle, this.as_deref(), now())
        .map_err(|e| e.to_string())?;
    print!("{}", render(&bundle, &report));
    Ok(())
}

fn render(bundle: &Bundle, r: &ImportReport) -> String {
    let mut out = String::new();
    let from = bundle.exported_from.as_deref().unwrap_or("another machine");
    out.push_str(&format!("Imported from {from}\n"));
    let list = |names: &[String]| names.join(", ");
    if !r.machines_added.is_empty() {
        out.push_str(&format!("  New scans: {}\n", list(&r.machines_added)));
    }
    if !r.machines_unchanged.is_empty() {
        out.push_str(&format!(
            "  Already up to date: {}\n",
            list(&r.machines_unchanged)
        ));
    }
    if r.skipped_own_machine {
        out.push_str("  Skipped this machine's own scan\n");
    }
    match &r.golden {
        GoldenImport::NotInBundle => {}
        GoldenImport::UpToDate => out.push_str("  Primary Rig: already up to date\n"),
        GoldenImport::Imported(v) => {
            let versions: Vec<_> = v.iter().map(|n| format!("v{n}")).collect();
            out.push_str(&format!("  Primary Rig: added {}\n", versions.join(", ")));
        }
        GoldenImport::Diverged => out.push_str(
            "  Primary Rig: both machines changed it, nothing imported. \
             Pick one with `primary promote` or `primary propose`\n",
        ),
    }
    if !r.profiles_added.is_empty() {
        out.push_str(&format!("  New profiles: {}\n", list(&r.profiles_added)));
    }
    if !r.profiles_conflicting.is_empty() {
        out.push_str(&format!(
            "  Kept your own version of profiles: {}\n",
            list(&r.profiles_conflicting)
        ));
    }
    out
}
