//! Ignore (decision #65): what the user doesn't want on a machine, from Bring
//! over, Get Up To Speed or Compare. Stored per machine (or for every linked
//! machine) in the store and synced in bundles; [`Store::target_for`] leaves
//! it out of the machine's target, so the score, the plan, Bring over and
//! drift alerts all stop counting it. Here: listing and changing them.

use std::collections::BTreeSet;

use patchpilot_core::{IgnoreEntry, Manifest, Matcher, PortablePath, TargetProfile};
use patchpilot_store::Store;
use serde::Serialize;

use crate::bring::{FileToBring, files_to_bring};

/// One ignore in force on a machine. Mirrored in `app/src/api.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IgnoreView {
    /// Identifies it for [`unignore`].
    pub key: String,
    pub label: String,
    /// Applies to every linked machine, not just this one.
    pub every_machine: bool,
    /// Primary Rig products it leaves out of this machine's target.
    pub hidden: usize,
    /// Files it keeps out of Bring over (when listed with [`bring_ignored`]).
    pub files: usize,
}

/// The ignores in force on `machine`, newest first, with how much of the
/// Primary Rig each one hides.
pub fn views(
    store: &Store,
    machine: &str,
    golden: Option<&Manifest>,
) -> Result<Vec<IgnoreView>, String> {
    let mut out: Vec<IgnoreView> = store
        .ignores_for(machine)
        .map_err(err)?
        .into_iter()
        .map(|e| IgnoreView {
            key: e.key(),
            hidden: golden.map_or(0, |g| hidden_products(g, &e.rules)),
            label: e.label,
            every_machine: e.machine_id.is_none(),
            files: 0,
        })
        .collect();
    out.reverse();
    Ok(out)
}

/// Bring over's ignored files on `machine` and the ignores that hide them:
/// what `source` would list through `base` (the target without ignores)
/// that it doesn't through `target`.
pub fn bring_ignored(
    store: &Store,
    machine: &str,
    source: &Manifest,
    base: &TargetProfile,
    target: &TargetProfile,
    scan: Option<&Manifest>,
) -> Result<(Vec<FileToBring>, Vec<IgnoreView>), String> {
    let kept: BTreeSet<_> = files_to_bring(source, target, scan)
        .into_iter()
        .map(|f| (f.root, f.rel))
        .collect();
    let files: Vec<FileToBring> = files_to_bring(source, base, scan)
        .into_iter()
        .filter(|f| !kept.contains(&(f.root.clone(), f.rel.clone())))
        .collect();
    let entries = store.ignores_for(machine).map_err(err)?;
    let mut views = views(store, machine, Some(source))?;
    for v in &mut views {
        let Some(e) = entries.iter().find(|e| e.key() == v.key) else {
            continue;
        };
        v.files = files
            .iter()
            .filter(|f| file_matches(source, f, &e.rules))
            .count();
    }
    views.retain(|v| v.files > 0);
    Ok((files, views))
}

/// Ignore what `rules` match on `machine`, or with `every_machine` on every
/// linked machine.
pub fn ignore(
    store: &mut Store,
    machine: &str,
    label: &str,
    rules: Vec<Matcher>,
    every_machine: bool,
    now: i64,
) -> Result<(), String> {
    if rules.is_empty() {
        return Ok(());
    }
    // One place at a time: moving it to every machine takes this machine's
    // own one back, and the other way round.
    for (every, undone) in [(!every_machine, true), (every_machine, false)] {
        store
            .set_ignore(&IgnoreEntry {
                machine_id: (!every).then(|| machine.to_owned()),
                label: label.to_owned(),
                rules: rules.clone(),
                at: now,
                undone,
            })
            .map_err(err)?;
    }
    Ok(())
}

/// Take back the ignore with this key on `machine`: its own one and any
/// that applies to every machine.
pub fn unignore(store: &mut Store, machine: &str, key: &str, now: i64) -> Result<(), String> {
    for e in store.ignores_for(machine).map_err(err)? {
        if e.key() == key {
            store
                .set_ignore(&IgnoreEntry {
                    at: now,
                    undone: true,
                    ..e
                })
                .map_err(err)?;
        }
    }
    Ok(())
}

/// A DAW Settings can stop watching on a machine (decision #97).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Daw {
    Reaper,
    Live,
}

impl Daw {
    pub const ALL: [Daw; 2] = [Daw::Reaper, Daw::Live];

    /// The ignore that turning it off adds, listed under Hidden items.
    pub fn label(self) -> &'static str {
        match self {
            Daw::Reaper => "Reaper (not used on this machine)",
            Daw::Live => "Ableton Live (not used on this machine)",
        }
    }

    /// Everything of the DAW's own folders: its files, templates and settings.
    pub fn rules(self) -> Vec<Matcher> {
        let roots: &[&str] = match self {
            Daw::Reaper => &[patchpilot_parity::reaper::REAPER_RESOURCES],
            Daw::Live => &[
                patchpilot_parity::ableton::ABLETON_USER_LIBRARY,
                patchpilot_parity::ableton::ABLETON_PREFS,
            ],
        };
        roots
            .iter()
            .map(|r| Matcher::PathGlob(format!("{r}/**")))
            .collect()
    }
}

/// Whether `machine` watches `daw`: no ignore of its folders is in force there.
pub fn daw_watched(store: &Store, machine: &str, daw: Daw) -> Result<bool, String> {
    let key = patchpilot_core::ignore_key(&daw.rules());
    Ok(!store
        .ignores_for(machine)
        .map_err(err)?
        .iter()
        .any(|e| e.key() == key))
}

/// Stop or start watching `daw` on `machine`. Off is an ignore of the DAW's
/// folders on this machine, so the score, Get Up To Speed, Bring over and
/// alerts stop asking for its files, and scans skip it; on takes it back.
pub fn set_daw_watched(
    store: &mut Store,
    machine: &str,
    daw: Daw,
    on: bool,
    now: i64,
) -> Result<(), String> {
    if on {
        unignore(
            store,
            machine,
            &patchpilot_core::ignore_key(&daw.rules()),
            now,
        )
    } else {
        ignore(store, machine, daw.label(), daw.rules(), false, now)
    }
}

/// Distinct Primary Rig products with an item one of `rules` matches.
fn hidden_products(golden: &Manifest, rules: &[Matcher]) -> usize {
    golden
        .items
        .iter()
        .filter(|i| {
            let product = i.product_id.as_ref().and_then(|id| golden.product(id));
            rules.iter().any(|r| r.matches(i, product))
        })
        .map(|i| match &i.product_id {
            Some(p) => p.0.clone(),
            None => format!("{:?}", i.path),
        })
        .collect::<BTreeSet<_>>()
        .len()
}

fn file_matches(golden: &Manifest, f: &FileToBring, rules: &[Matcher]) -> bool {
    golden.items.iter().any(|i| {
        matches!(&i.path, PortablePath::Logical { root, rel }
            if *root == f.root && rel.eq_ignore_ascii_case(&f.rel))
            && rules
                .iter()
                .any(|r| r.matches(i, i.product_id.as_ref().and_then(|id| golden.product(id))))
    })
}

fn err(e: patchpilot_store::StoreError) -> String {
    e.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use patchpilot_core::{Arch, Item, MachineInfo, Os, PathMap, PluginFormat, Product, ProductId};

    fn rig(name: &str, products: &[&str]) -> Manifest {
        let mut m = Manifest::new(
            MachineInfo {
                id: name.into(),
                name: name.into(),
                os: Os::Windows,
                arch: Arch::X86_64,
                os_version: None,
                path_map: PathMap::new(Os::Windows),
                disks: Vec::new(),
            },
            1,
        );
        for p in products {
            m.products.push(Product {
                canonical_id: ProductId(p.to_string()),
                vendor: "Acme".into(),
                name: p.to_string(),
            });
            let mut item = Item::plugin(
                PluginFormat::Vst3,
                Os::Windows,
                PortablePath::logical("VST3_SYSTEM", format!("{p}.vst3")),
            );
            item.product_id = Some(ProductId(p.to_string()));
            m.items.push(item);
        }
        m
    }

    #[test]
    fn ignored_products_leave_the_score_and_come_back() {
        let golden = rig("studio", &["acme/a", "acme/b"]);
        let laptop = rig("laptop", &["acme/a"]);
        let mut store = Store::open_in_memory().unwrap();
        crate::save_scan(&mut store, &golden).unwrap();
        store.save_golden(&golden, Some("studio"), None, 1).unwrap();
        crate::save_scan(&mut store, &laptop).unwrap();
        let score = |s: &Store| {
            let g = s.golden(None).unwrap().unwrap().manifest;
            patchpilot_parity::compare(&g, &s.target_for("laptop").unwrap(), &laptop).score
        };
        assert!(score(&store) < 100);

        let rules = vec![Matcher::Product(ProductId("acme/b".into()))];
        ignore(&mut store, "laptop", "B", rules.clone(), false, 2).unwrap();
        assert_eq!(score(&store), 100);
        let v = views(&store, "laptop", Some(&golden)).unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(
            (v[0].label.as_str(), v[0].hidden, v[0].every_machine),
            ("B", 1, false)
        );
        // Only the laptop's.
        assert!(views(&store, "studio", None).unwrap().is_empty());

        // Moving it to every machine keeps one entry.
        ignore(&mut store, "laptop", "B", rules, true, 3).unwrap();
        let v = views(&store, "laptop", None).unwrap();
        assert_eq!(v.len(), 1);
        assert!(v[0].every_machine);
        assert_eq!(views(&store, "studio", None).unwrap().len(), 1);

        unignore(&mut store, "laptop", &v[0].key, 4).unwrap();
        assert!(views(&store, "laptop", None).unwrap().is_empty());
        assert!(score(&store) < 100);
    }

    #[test]
    fn ignored_product_on_the_machine_is_not_a_suggestion() {
        let golden = rig("studio", &["acme/a", "acme/b"]);
        let laptop = rig("laptop", &["acme/a", "acme/b"]);
        let mut store = Store::open_in_memory().unwrap();
        crate::save_scan(&mut store, &golden).unwrap();
        store.save_golden(&golden, Some("studio"), None, 1).unwrap();
        crate::save_scan(&mut store, &laptop).unwrap();
        let rules = vec![Matcher::Product(ProductId("acme/b".into()))];
        ignore(&mut store, "laptop", "B", rules, false, 2).unwrap();
        let c = patchpilot_parity::compare(&golden, &store.target_for("laptop").unwrap(), &laptop);
        assert_eq!(patchpilot_parity::suggestions(&c).count(), 0);
    }

    #[test]
    fn a_daw_turned_off_leaves_the_score_and_comes_back() {
        let mut golden = rig("studio", &["acme/a"]);
        let laptop = rig("laptop", &["acme/a"]);
        let mut template = Item::plugin(
            PluginFormat::Vst3,
            Os::Windows,
            PortablePath::logical(
                patchpilot_parity::reaper::REAPER_RESOURCES,
                "TrackTemplates/Drums.RTrackTemplate",
            ),
        );
        template.kind = patchpilot_core::ItemKind::DawAsset;
        template.format = None;
        template.product_id = Some(ProductId("reaper-template/Drums.RTrackTemplate".into()));
        golden.products.push(Product {
            canonical_id: ProductId("reaper-template/Drums.RTrackTemplate".into()),
            vendor: "Reaper".into(),
            name: "Drums".into(),
        });
        golden.items.push(template);
        let mut store = Store::open_in_memory().unwrap();
        crate::save_scan(&mut store, &golden).unwrap();
        store.save_golden(&golden, Some("studio"), None, 1).unwrap();
        crate::save_scan(&mut store, &laptop).unwrap();
        let score = |s: &Store| {
            let g = s.golden(None).unwrap().unwrap().manifest;
            patchpilot_parity::compare(&g, &s.target_for("laptop").unwrap(), &laptop).score
        };
        assert!(score(&store) < 100);
        assert!(daw_watched(&store, "laptop", Daw::Reaper).unwrap());

        set_daw_watched(&mut store, "laptop", Daw::Reaper, false, 2).unwrap();
        assert_eq!(score(&store), 100);
        assert!(!daw_watched(&store, "laptop", Daw::Reaper).unwrap());
        assert!(daw_watched(&store, "laptop", Daw::Live).unwrap());
        assert!(daw_watched(&store, "studio", Daw::Reaper).unwrap());

        set_daw_watched(&mut store, "laptop", Daw::Reaper, true, 3).unwrap();
        assert!(score(&store) < 100);
        assert!(daw_watched(&store, "laptop", Daw::Reaper).unwrap());
    }
}
