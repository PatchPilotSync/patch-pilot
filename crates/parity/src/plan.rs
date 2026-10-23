//! "Bring this machine up to speed" v1 (#39, strategy sections 10.6 and 17):
//! turn a diff into an ordered, honest checklist.
//!
//! Read-only. Nothing here installs, copies or writes anything, and plug-ins
//! are never copied between machines (CLAUDE.md rule 2): a missing plug-in is
//! always installed through its vendor's app or installer.

use std::collections::BTreeMap;

use patchpilot_core::{ItemKind, Manifest, ProductId, TargetProfile};

use crate::diff::{ParityState, ProductDiff, diff_manifests};
use crate::score::apply_profile;

/// Plan phases, in the order they run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Phase {
    /// Licensing tools and vendor apps that later steps depend on.
    Foundation,
    /// "Open Native Access and install these 4."
    VendorHandoff,
    /// Plug-ins with no known vendor app, and DAW files.
    Manual,
}

impl Phase {
    pub fn title(self) -> &'static str {
        match self {
            Self::Foundation => "Foundation",
            Self::VendorHandoff => "Vendor apps",
            Self::Manual => "Manual steps",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    /// 1-based, in plan order.
    pub number: usize,
    pub phase: Phase,
    pub title: String,
    /// One line per product, e.g. `Kontakt 7 7.10.1 (you have 7.9.0)`.
    pub details: Vec<String>,
    pub products: Vec<ProductId>,
    /// Step numbers that must be done first.
    pub after: Vec<usize>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    pub steps: Vec<Step>,
}

/// A vendor app that installs other vendors' products, keyed by the app's
/// product ID (`app/<adapter id>` from the scanner's vendor-app detection).
struct Manager {
    app: &'static str,
    name: &'static str,
    /// Lowercase substrings of product vendor names it manages.
    vendors: &'static [&'static str],
}

const MANAGERS: &[Manager] = &[
    Manager {
        app: "app/native-access",
        name: "Native Access",
        vendors: &["native instruments"],
    },
    Manager {
        app: "app/waves-central",
        name: "Waves Central",
        vendors: &["waves"],
    },
    Manager {
        app: "app/ua-connect",
        name: "UA Connect",
        vendors: &["universal audio"],
    },
    Manager {
        app: "app/arturia-software-center",
        name: "Arturia Software Center",
        vendors: &["arturia"],
    },
    Manager {
        app: "app/izotope-product-portal",
        name: "iZotope Product Portal",
        vendors: &["izotope"],
    },
    Manager {
        app: "app/plugin-alliance-installation-manager",
        name: "Plugin Alliance Installation Manager",
        vendors: &["plugin alliance", "brainworx"],
    },
    Manager {
        app: "app/spitfire-audio",
        name: "Spitfire Audio App",
        vendors: &["spitfire"],
    },
];

/// Diff `machine` against `golden` narrowed by `profile` and plan the steps.
/// Run [`crate::align_products`] first.
pub fn up_to_speed(golden: &Manifest, profile: &TargetProfile, machine: &Manifest) -> Plan {
    let target = apply_profile(golden, profile);
    let diff = diff_manifests(&target, machine);
    let has = |id: &str| {
        machine
            .items
            .iter()
            .any(|i| i.product_id.as_ref().is_some_and(|p| p.0 == id))
    };
    let kind_of = |id: &ProductId| {
        target
            .items
            .iter()
            .find(|i| i.product_id.as_ref() == Some(id))
            .map_or(ItemKind::Plugin, |i| i.kind)
    };
    let vendor_of = |id: &ProductId| {
        target
            .product(id)
            .map(|p| p.vendor.clone())
            .unwrap_or_default()
    };

    let todo: Vec<&ProductDiff> = diff
        .products
        .iter()
        .filter(|p| {
            matches!(
                p.state,
                ParityState::Missing | ParityState::Outdated | ParityState::Incomplete
            )
        })
        .collect();

    let mut steps: Vec<Step> = Vec::new();
    let mut push = |phase, title: String, items: &[&ProductDiff], after: Vec<usize>| {
        let number = steps.len() + 1;
        let mut items = items.to_vec();
        items.sort_by(|a, b| a.name.cmp(&b.name));
        steps.push(Step {
            number,
            phase,
            title,
            details: items.iter().map(|p| detail(p)).collect(),
            products: items.iter().map(|p| p.product_id.clone()).collect(),
            after,
        });
        number
    };

    // Foundation: licensing first (PACE/iLok before anything that needs it),
    // then vendor apps.
    let mut license_steps = Vec::new();
    for p in todo
        .iter()
        .filter(|p| kind_of(&p.product_id) == ItemKind::LicenseTool)
    {
        let n = push(
            Phase::Foundation,
            format!("{} {}", verb(p), p.name),
            &[p],
            Vec::new(),
        );
        license_steps.push(n);
    }
    let mut app_steps: BTreeMap<String, usize> = BTreeMap::new();
    for p in todo
        .iter()
        .filter(|p| kind_of(&p.product_id) == ItemKind::VendorApp)
    {
        let n = push(
            Phase::Foundation,
            format!("{} {}", verb(p), p.name),
            &[p],
            license_steps.clone(),
        );
        app_steps.insert(p.product_id.0.clone(), n);
    }

    // Plug-ins grouped by the vendor app that installs them.
    let plugins: Vec<&ProductDiff> = todo
        .iter()
        .copied()
        .filter(|p| kind_of(&p.product_id) == ItemKind::Plugin)
        .collect();
    let mut by_manager: BTreeMap<&str, Vec<&ProductDiff>> = BTreeMap::new();
    let mut by_vendor: BTreeMap<String, Vec<&ProductDiff>> = BTreeMap::new();
    for p in &plugins {
        let vendor = vendor_of(&p.product_id);
        match manager_for(&vendor) {
            Some(m) => by_manager.entry(m.name).or_default().push(p),
            None => by_vendor.entry(vendor).or_default().push(p),
        }
    }
    for m in MANAGERS {
        let Some(items) = by_manager.get(m.name) else {
            continue;
        };
        let mut after = license_steps.clone();
        match app_steps.get(m.app) {
            Some(&n) => after.push(n),
            None if !has(m.app) => {
                // Golden doesn't list the app, but the machine needs it.
                let n = push(
                    Phase::Foundation,
                    format!("Install {}", m.name),
                    &[],
                    license_steps.clone(),
                );
                add_dep(&mut after, n);
            }
            None => {}
        }
        push(
            Phase::VendorHandoff,
            format!("Open {} and {}", m.name, count_phrase(items)),
            items,
            after,
        );
    }
    for (vendor, items) in &by_vendor {
        let from = if vendor.is_empty() {
            "the vendor's installer".to_owned()
        } else {
            format!("{vendor}'s installer or website")
        };
        push(
            Phase::Manual,
            format!("Install from {from}: {}", count_phrase(items)),
            items,
            license_steps.clone(),
        );
    }

    // DAW files and configs are user-owned and will sync later; for now the
    // user copies them.
    let files: Vec<&ProductDiff> = todo
        .iter()
        .copied()
        .filter(|p| {
            matches!(
                kind_of(&p.product_id),
                ItemKind::DawAsset | ItemKind::Config | ItemKind::Folder
            )
        })
        .collect();
    if !files.is_empty() {
        push(
            Phase::Manual,
            format!(
                "Copy {} DAW file{} from the Golden machine (file sync comes later)",
                files.len(),
                if files.len() == 1 { "" } else { "s" }
            ),
            &files,
            Vec::new(),
        );
    }

    // Steps were pushed phase by phase except for late "Install <app>"
    // steps; sort by phase and renumber, keeping dependencies.
    renumber(&mut steps);
    Plan { steps }
}

fn add_dep(after: &mut Vec<usize>, n: usize) {
    if !after.contains(&n) {
        after.push(n);
    }
}

/// Order steps by phase (stable) and renumber `number` and `after`.
fn renumber(steps: &mut [Step]) {
    steps.sort_by_key(|s| (s.phase, s.number));
    let map: BTreeMap<usize, usize> = steps
        .iter()
        .enumerate()
        .map(|(i, s)| (s.number, i + 1))
        .collect();
    for s in steps.iter_mut() {
        s.number = map[&s.number];
        for a in &mut s.after {
            *a = map[a];
        }
        s.after.sort_unstable();
        s.after.dedup();
    }
}

fn manager_for(vendor: &str) -> Option<&'static Manager> {
    let v = vendor.to_lowercase();
    MANAGERS
        .iter()
        .find(|m| m.vendors.iter().any(|needle| v.contains(needle)))
}

fn verb(p: &ProductDiff) -> &'static str {
    match p.state {
        ParityState::Outdated => "Update",
        _ => "Install",
    }
}

/// "install 2 and update 1" for a group of products.
fn count_phrase(items: &[&ProductDiff]) -> String {
    let updates = items
        .iter()
        .filter(|p| p.state == ParityState::Outdated)
        .count();
    let installs = items.len() - updates;
    match (installs, updates) {
        (i, 0) => format!("install {i}"),
        (0, u) => format!("update {u}"),
        (i, u) => format!("install {i} and update {u}"),
    }
}

/// `Acme Comp 3.0.1 (you have 3.0)`, `Acme Verb: add AAX`.
fn detail(p: &ProductDiff) -> String {
    match p.state {
        ParityState::Incomplete => {
            let formats: Vec<_> = p
                .missing_formats
                .iter()
                .map(|f| format!("{f:?}").to_uppercase())
                .collect();
            format!("{}: add {}", p.name, formats.join(", "))
        }
        ParityState::Outdated => format!(
            "{} {} (you have {})",
            p.name,
            p.golden_version.as_deref().unwrap_or("?"),
            p.machine_version.as_deref().unwrap_or("?")
        ),
        _ => match &p.golden_version {
            Some(v) => format!("{} {v}", p.name),
            None => p.name.clone(),
        },
    }
}
