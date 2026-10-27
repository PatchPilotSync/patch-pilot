//! "Bring this machine up to speed" v1 (#39, strategy sections 10.6 and 17):
//! turn a diff into an ordered, honest checklist.
//!
//! Read-only. Nothing here installs, copies or writes anything, and plug-ins
//! are never copied between machines (CLAUDE.md rule 2): a missing plug-in is
//! always installed through its vendor's app or installer.

use std::collections::BTreeMap;

use patchpilot_core::{ItemKind, Manifest, PluginFormat, ProductId, TargetProfile};

use crate::diff::{ParityState, ProductDiff, diff_manifests};
use crate::score::apply_profile;

/// Plan phases, in the order they run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
#[serde(rename_all = "snake_case")]
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

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Step {
    /// 1-based, in plan order.
    pub number: usize,
    pub phase: Phase,
    pub title: String,
    /// One line per product, e.g. `Kontakt 7: 7.9.0 here, Primary Rig has 7.10.1`.
    pub details: Vec<String>,
    /// Per product: not here at all, or here at another version.
    pub needs: Vec<Need>,
    pub products: Vec<ProductId>,
    /// Step numbers that must be done first.
    pub after: Vec<usize>,
    /// The vendor app or licensing tool this step uses (scanner adapter ID,
    /// e.g. `native-access`), so the app can offer to open it.
    pub app: Option<String>,
    /// The vendor's website, from the catalog, for plug-ins installed from
    /// the vendor's own downloads.
    pub link: Option<String>,
    /// True when the step's plug-ins have no known vendor, so each is best
    /// found by searching for it.
    pub search: bool,
}

/// What a step's product needs on the machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Need {
    /// Not installed in any format.
    Install,
    /// Installed, older than the Primary Rig's.
    Update,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct Plan {
    pub steps: Vec<Step>,
    /// Plug-ins the machine has in other formats than the Primary Rig's
    /// (a VST3 where the Primary Rig has a VST2). They count as installed, so
    /// no step asks for them, but the difference is worth knowing.
    pub other_formats: Vec<OtherFormats>,
}

/// A plug-in installed in some formats, not all of the Primary Rig's.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct OtherFormats {
    pub product_id: ProductId,
    pub name: String,
    /// Formats on the machine, e.g. `["VST3", "AU"]`.
    pub here: Vec<String>,
    /// The Primary Rig's formats the machine lacks, e.g. `["VST2"]`.
    pub missing: Vec<String>,
}

/// A vendor app that installs other vendors' products, keyed by the app's
/// product ID (`app/<adapter id>` from the scanner's vendor-app detection).
struct Manager {
    app: &'static str,
    name: &'static str,
    /// Lowercase vendor names it manages, matched as whole words of the
    /// product's vendor ("waves" is in "Waves Audio Ltd", not "Wavesfactory").
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

    // A plug-in in any format counts as installed (a Mac with the VST3 of a
    // PC's VST2): it goes in `other_formats`, not in a step.
    let todo: Vec<&ProductDiff> = diff
        .products
        .iter()
        .filter(|p| matches!(p.state, ParityState::Missing | ParityState::Outdated))
        .collect();
    let other_formats = diff
        .products
        .iter()
        .filter(|p| p.state == ParityState::Incomplete)
        .map(|p| {
            let mut here: Vec<PluginFormat> = machine
                .items
                .iter()
                .filter(|i| i.product_id.as_ref() == Some(&p.product_id))
                .filter_map(|i| i.format)
                .collect();
            here.sort_by_key(|f| format_order(*f));
            here.dedup();
            OtherFormats {
                product_id: p.product_id.clone(),
                name: p.name.clone(),
                here: here.into_iter().map(format_label).collect(),
                missing: p
                    .missing_formats
                    .iter()
                    .copied()
                    .map(format_label)
                    .collect(),
            }
        })
        .collect();

    let mut steps: Vec<Step> = Vec::new();
    let mut push =
        |phase, title: String, items: &[&ProductDiff], after: Vec<usize>, app: Option<&str>| {
            let number = steps.len() + 1;
            let mut items = items.to_vec();
            items.sort_by(|a, b| a.name.cmp(&b.name));
            steps.push(Step {
                number,
                phase,
                title,
                details: items.iter().map(|p| detail(p)).collect(),
                needs: items
                    .iter()
                    .map(|p| match p.state {
                        ParityState::Outdated => Need::Update,
                        _ => Need::Install,
                    })
                    .collect(),
                products: items.iter().map(|p| p.product_id.clone()).collect(),
                after,
                app: app.and_then(|a| a.strip_prefix("app/")).map(str::to_owned),
                link: None,
                search: false,
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
            Some(&p.product_id.0),
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
            Some(&p.product_id.0),
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
                // Primary Rig doesn't list the app, but the machine needs it.
                let n = push(
                    Phase::Foundation,
                    format!("Install {}", m.name),
                    &[],
                    license_steps.clone(),
                    Some(m.app),
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
            Some(m.app),
        );
    }
    // Where each vendor's plug-ins come from: (step number, website, search).
    let catalog = patchpilot_catalog::current();
    let mut sources = Vec::new();
    for (vendor, items) in &by_vendor {
        let title = if vendor.is_empty() {
            format!("Find each one's installer: {}", count_phrase(items))
        } else {
            format!("Install from {vendor}: {}", count_phrase(items))
        };
        let n = push(Phase::Manual, title, items, license_steps.clone(), None);
        let link = catalog.vendor_named(vendor).and_then(|v| v.website.clone());
        sources.push((n, link, vendor.is_empty()));
    }

    // ReaPack installs its own packages once its repositories are there, and
    // Reaper's folder settings are pointed at this machine's folders; both
    // are one click in the app (decision #52).
    let reapack: Vec<&ProductDiff> = todo
        .iter()
        .copied()
        .filter(|p| crate::reapack::is_reapack(&p.product_id))
        .collect();
    if !reapack.is_empty() {
        push(
            Phase::Manual,
            format!(
                "Restore {} (Reaper must be closed)",
                count_noun(reapack.len(), "ReaPack item")
            ),
            &reapack,
            Vec::new(),
            None,
        );
    }
    let folders: Vec<&ProductDiff> = todo
        .iter()
        .copied()
        .filter(|p| crate::reaper_folders::is_reaper_folder(&p.product_id))
        .collect();
    if !folders.is_empty() {
        push(
            Phase::Manual,
            format!(
                "Set {} to this machine's folders (Reaper must be closed)",
                count_noun(folders.len(), "Reaper folder setting")
            ),
            &folders,
            Vec::new(),
            None,
        );
    }

    // DAW files and configs are user-owned; linked machines bring them over
    // in one click (decision #45).
    let files: Vec<&ProductDiff> = todo
        .iter()
        .copied()
        .filter(|p| {
            matches!(
                kind_of(&p.product_id),
                ItemKind::DawAsset | ItemKind::Config | ItemKind::Folder
            ) && !crate::reapack::is_reapack(&p.product_id)
                && !crate::reaper_folders::is_reaper_folder(&p.product_id)
        })
        .collect();
    if !files.is_empty() {
        push(
            Phase::Manual,
            format!(
                "Bring over {} DAW file{} from the Primary Rig",
                files.len(),
                if files.len() == 1 { "" } else { "s" }
            ),
            &files,
            Vec::new(),
            None,
        );
    }

    for (n, link, search) in sources {
        steps[n - 1].link = link;
        steps[n - 1].search = search;
    }
    // Steps were pushed phase by phase except for late "Install <app>"
    // steps; sort by phase and renumber, keeping dependencies.
    renumber(&mut steps);
    Plan {
        steps,
        other_formats,
    }
}

/// `VST3`, `AU`, `CLAP`.
fn format_label(f: PluginFormat) -> String {
    format!("{f:?}").to_uppercase()
}

/// VST3 first, as the format most hosts on both OSes load.
fn format_order(f: PluginFormat) -> u8 {
    match f {
        PluginFormat::Vst3 => 0,
        PluginFormat::Au => 1,
        PluginFormat::Clap => 2,
        PluginFormat::Vst2 => 3,
        PluginFormat::Aax => 4,
        PluginFormat::Jsfx => 5,
    }
}

/// `1 ReaPack item`, `3 ReaPack items`.
fn count_noun(n: usize, noun: &str) -> String {
    format!("{n} {noun}{}", if n == 1 { "" } else { "s" })
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
    let words: Vec<String> = vendor
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect();
    MANAGERS.iter().find(|m| {
        m.vendors.iter().any(|needle| {
            let needle: Vec<&str> = needle.split(' ').collect();
            words.windows(needle.len()).any(|w| w == needle.as_slice())
        })
    })
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

/// `Acme Comp: 3.0 here, Primary Rig has 3.0.1`, `Acme Verb 1.2`.
fn detail(p: &ProductDiff) -> String {
    match p.state {
        ParityState::Outdated => format!(
            "{}: {} here, Primary Rig has {}",
            p.name,
            p.machine_version.as_deref().unwrap_or("?"),
            p.golden_version.as_deref().unwrap_or("?")
        ),
        _ => match &p.golden_version {
            Some(v) => format!("{} {v}", p.name),
            None => p.name.clone(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vendor_apps_match_whole_vendor_words() {
        let app = |v: &str| manager_for(v).map(|m| m.app);
        assert_eq!(app("Waves"), Some("app/waves-central"));
        assert_eq!(app("Waves Audio Ltd."), Some("app/waves-central"));
        assert_eq!(app("Wavesfactory"), None);
        assert_eq!(app("Native Instruments GmbH"), Some("app/native-access"));
        assert_eq!(
            app("Brainworx"),
            Some("app/plugin-alliance-installation-manager")
        );
        assert_eq!(
            app("Plugin Alliance"),
            Some("app/plugin-alliance-installation-manager")
        );
        assert_eq!(app("Acme Audio"), None);
    }

    #[test]
    fn manual_steps_say_where_plugins_come_from() {
        use patchpilot_core::{
            Arch, Item, MachineInfo, Os, PathMap, PluginFormat, PortablePath, Product,
        };
        let manifest = |plugins: &[(&str, &str)]| {
            let mut m = Manifest::new(
                MachineInfo {
                    id: "m".into(),
                    name: "m".into(),
                    os: Os::Macos,
                    arch: Arch::Aarch64,
                    os_version: None,
                    path_map: PathMap::new(Os::Macos),
                },
                0,
            );
            for (id, vendor) in plugins {
                m.products.push(Product {
                    canonical_id: (*id).into(),
                    vendor: (*vendor).into(),
                    name: id.split('/').next_back().unwrap().into(),
                });
                let mut item = Item::plugin(
                    PluginFormat::Vst3,
                    Os::Macos,
                    PortablePath::logical("VST3_SYSTEM", *id),
                );
                item.product_id = Some((*id).into());
                m.items.push(item);
            }
            m
        };
        let golden = manifest(&[("fabfilter/pro-c-3", "FabFilter"), ("unknown/mystery", "")]);
        let plan = up_to_speed(&golden, &TargetProfile::default(), &manifest(&[]));
        let rows: Vec<_> = plan
            .steps
            .iter()
            .map(|s| (s.title.as_str(), s.link.as_deref(), s.search))
            .collect();
        assert_eq!(
            rows,
            [
                ("Find each one's installer: install 1", None, true),
                (
                    "Install from FabFilter: install 1",
                    Some("https://www.fabfilter.com"),
                    false
                ),
            ]
        );
    }
}
