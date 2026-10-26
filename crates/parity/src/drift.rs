//! Drift alerts and suggestions for the Primary Rig (M9, strategy points 3 and 7).
//!
//! A **gap** is something the Primary Rig has that a machine lacks or has an
//! older version of. A **suggestion** is the other way round: a product the
//! machine has that Primary Rig lacks, or has an older version of, which the user
//! may want to add to Primary Rig ("propose to Primary Rig").
//!
//! [`alerts`] compares what each machine looks like now with what the user
//! was last told about ([`Seen`]) and returns only what is new, so an alert
//! never repeats. Pure: the caller stores `Seen` and shows the alerts.

use std::collections::{BTreeMap, BTreeSet};

use patchpilot_core::{ItemKind, Manifest, ProductId};
use serde::{Deserialize, Serialize};

use crate::reapack::is_reapack;
use crate::{Comparison, ComparisonRow, ParityState};

/// Rows of `c` that could go into the Primary Rig: products Primary Rig doesn't
/// have, or has an older version of. Folders and configs are left out (a
/// sample library or a settings file isn't something to add from one
/// machine), except ReaPack repositories, which its packages need. So is
/// what the Primary Rig has but the machine's target leaves out.
pub fn suggestions(c: &Comparison) -> impl Iterator<Item = &ComparisonRow> {
    c.rows.iter().filter(|r| {
        matches!(r.state, ParityState::Extra | ParityState::NewerThanGolden)
            && !r.left_out
            && (!matches!(r.kind, ItemKind::Folder | ItemKind::Config) || is_reapack(&r.product_id))
    })
}

/// What kind of thing a suggestion is, so a long list reads as "12 plug-ins,
/// 40 scripts, 3 templates" instead of one big number. In display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SuggestionGroup {
    Plugins,
    Apps,
    Scripts,
    Templates,
    Presets,
    Themes,
    Configs,
    Other,
}

impl SuggestionGroup {
    pub fn of(r: &ComparisonRow) -> Self {
        let id = r.product_id.0.as_str();
        let under = |p: &str| id.starts_with(p);
        if under("reaper-script/") || is_reapack(&r.product_id) {
            Self::Scripts
        } else if under("reaper-template/") {
            Self::Templates
        } else if under("reaper-theme/") {
            Self::Themes
        } else if under("reaper-preset/") {
            Self::Presets
        } else if under(crate::ableton::PRODUCT_PREFIX) {
            match r.kind {
                ItemKind::Config => Self::Configs,
                _ if r.name.to_ascii_lowercase().ends_with(".als") => Self::Templates,
                _ => Self::Presets,
            }
        } else if under(crate::presets::PRODUCT_PREFIX) {
            Self::Presets
        } else {
            match r.kind {
                ItemKind::Plugin => Self::Plugins,
                ItemKind::VendorApp | ItemKind::LicenseTool => Self::Apps,
                ItemKind::Config => Self::Configs,
                _ => Self::Other,
            }
        }
    }
}

/// What counts once among suggestions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SuggestionUnit {
    pub group: SuggestionGroup,
    /// Rows with the same key are one thing.
    pub key: String,
    /// Its name: the product, the script's folder, or "ReaPack packages".
    pub label: String,
}

/// Reaper files that only work together on one machine
/// ([`crate::reaper_links::units`]): each linked file's product mapped to the
/// path of its unit's first file, so a script and the library it loads from
/// another folder count once. Empty for scans made before links were
/// recorded; folders alone then decide.
#[derive(Debug, Clone, Default)]
pub struct Packages(BTreeMap<ProductId, String>);

impl Packages {
    pub fn of(m: &Manifest) -> Self {
        let names: BTreeMap<&ProductId, &str> = m
            .products
            .iter()
            .map(|p| (&p.canonical_id, p.name.as_str()))
            .collect();
        Self(
            crate::reaper_links::units(m)
                .into_iter()
                .filter(|(file, unit)| file != unit)
                .filter_map(|(file, unit)| Some((file, (*names.get(&unit)?).to_owned())))
                .collect(),
        )
    }
}

/// The [`SuggestionUnit`] a suggestion row belongs to.
pub fn suggestion_unit(r: &ComparisonRow, packages: &Packages) -> SuggestionUnit {
    let (key, label) = unit_of(r, packages);
    SuggestionUnit {
        group: SuggestionGroup::of(r),
        key,
        label,
    }
}

/// What counts once among suggestions, and its name: all of ReaPack is one
/// thing (it installs them itself), and a Reaper script or theme counts with
/// the rest of its folder, since script packages put many files in one
/// folder (the folder of its package's first file, when [`Packages`] links
/// it to files elsewhere). Other rows count on their own.
fn unit_of(r: &ComparisonRow, packages: &Packages) -> (String, String) {
    if is_reapack(&r.product_id) {
        return ("reapack".to_owned(), REAPACK_GROUP.to_owned());
    }
    let id = &r.product_id.0;
    if id.starts_with("reaper-script/") || id.starts_with("reaper-theme/") {
        // The row's name is the file's path under the resource folder:
        // `Scripts/ReaTeam Scripts/Items Editing/foo.lua`. A file right in
        // `Scripts/` is its own unit; a theme's images sit in a folder named
        // like its `.ReaperTheme` file.
        let path = packages.0.get(&r.product_id).unwrap_or(&r.name);
        if let Some((dir, file)) = path.rsplit_once('/') {
            let (unit, label) = match dir.split_once('/') {
                Some(_) => (dir.to_owned(), dir.rsplit('/').next().unwrap_or(dir)),
                None => {
                    let stem = file.split('.').next().unwrap_or(file);
                    (format!("{dir}/{stem}"), stem)
                }
            };
            return (format!("dir:{unit}"), label.to_owned());
        }
    }
    (format!("row:{id}"), r.name.clone())
}

/// How many suggestions there are in each group, counting each
/// [`SuggestionUnit`] once. Groups without any are left out.
pub fn suggestion_summary<'a>(
    rows: impl IntoIterator<Item = &'a ComparisonRow>,
    packages: &Packages,
) -> Vec<(SuggestionGroup, usize)> {
    let mut units: BTreeMap<SuggestionGroup, BTreeSet<String>> = BTreeMap::new();
    for r in rows {
        units
            .entry(SuggestionGroup::of(r))
            .or_default()
            .insert(unit_of(r, packages).0);
    }
    units.into_iter().map(|(g, u)| (g, u.len())).collect()
}

/// ReaPack can bring thousands of scripts; alerts name them as one thing.
const REAPACK_GROUP: &str = "ReaPack packages";

/// Identifies a suggestion across scans and machines: the product and the
/// machine's version, so ignoring Serum 2.0 still suggests Serum 2.1 later.
pub fn suggestion_key(product_id: &str, version: Option<&str>) -> String {
    format!("{product_id}@{}", version.unwrap_or_default())
}

fn row_suggestion_key(r: &ComparisonRow) -> String {
    suggestion_key(&r.product_id.0, r.machine_version.as_deref())
}

/// Rows of `c` where the machine falls short of Primary Rig.
pub fn gaps(c: &Comparison) -> impl Iterator<Item = &ComparisonRow> {
    c.rows.iter().filter(|r| {
        matches!(
            r.state,
            ParityState::Missing | ParityState::Incomplete | ParityState::Outdated
        )
    })
}

/// A gap is new when the product is short of a Primary Rig version it wasn't short
/// of before, or when it's now missing outright; installing an older version
/// of a missing plug-in isn't news.
fn gap_keys(r: &ComparisonRow) -> Vec<String> {
    let key = format!(
        "{}@{}",
        r.product_id.0,
        r.golden_version.as_deref().unwrap_or_default()
    );
    if r.state == ParityState::Missing {
        vec![format!("{key}!missing"), key]
    } else {
        vec![key]
    }
}

/// One machine as it compares now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observed {
    pub machine_id: String,
    pub machine_name: String,
    pub is_this_machine: bool,
    pub score: u8,
    /// `(keys, sentence)`, e.g. "Serum is missing".
    gaps: Vec<(Vec<String>, String)>,
    /// `(key, product name)`, without the ones the user ignored.
    suggestions: Vec<(String, String)>,
}

impl Observed {
    /// `ignored` holds [`suggestion_key`]s the user chose not to add.
    pub fn new(
        machine_id: &str,
        machine_name: &str,
        is_this_machine: bool,
        c: &Comparison,
        ignored: &BTreeSet<String>,
    ) -> Self {
        Self {
            machine_id: machine_id.to_owned(),
            machine_name: machine_name.to_owned(),
            is_this_machine,
            score: c.score,
            gaps: gaps(c).map(|r| (gap_keys(r), gap_sentence(r))).collect(),
            suggestions: suggestions(c)
                .map(|r| (row_suggestion_key(r), unit_of(r, &Packages::default()).1))
                .filter(|(k, _)| !ignored.contains(k))
                .collect(),
        }
    }
}

fn gap_sentence(r: &ComparisonRow) -> String {
    if is_reapack(&r.product_id) {
        return format!("{REAPACK_GROUP} are missing or out of date");
    }
    match r.state {
        ParityState::Missing => format!("{} is missing", r.name),
        ParityState::Outdated => format!("{} is out of date", r.name),
        _ => format!("{} lacks a format", r.name),
    }
}

/// What the user has already been told, per machine. Kept between runs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Seen {
    #[serde(default)]
    machines: BTreeMap<String, SeenMachine>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct SeenMachine {
    #[serde(default)]
    gaps: BTreeSet<String>,
    #[serde(default)]
    suggestions: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Alert {
    /// A machine fell further from the Primary Rig.
    Drift {
        machine_id: String,
        machine: String,
        score: u8,
        /// New gaps, as sentences.
        items: Vec<String>,
    },
    /// This machine has something new the Primary Rig doesn't.
    Suggest {
        machine_id: String,
        machine: String,
        products: Vec<String>,
    },
}

impl Alert {
    pub fn title(&self) -> String {
        match self {
            Self::Drift { machine, .. } => format!("{machine} drifted from the Primary Rig"),
            Self::Suggest {
                machine, products, ..
            } => match products.as_slice() {
                [one] => format!("New on {machine}: {one}"),
                many => format!("{} new things on {machine}", many.len()),
            },
        }
    }

    pub fn body(&self) -> String {
        match self {
            Self::Drift { score, items, .. } => {
                format!("{}. Parity {score}.", sentence_list(items))
            }
            Self::Suggest { products, .. } => {
                let what = if products.len() == 1 {
                    "It isn't"
                } else {
                    "They aren't"
                };
                let list = if products.len() > 1 {
                    format!("{}. ", sentence_list(products))
                } else {
                    String::new()
                };
                format!("{list}{what} in the Primary Rig yet. Open Compare to add or ignore.")
            }
        }
    }
}

/// "A, B and 3 more", capitalised.
fn sentence_list(items: &[String]) -> String {
    let shown = &items[..items.len().min(2)];
    let mut s = shown.join(", ");
    match items.len() - shown.len() {
        0 => {}
        n => s.push_str(&format!(" and {n} more")),
    }
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => s,
    }
}

/// What's new since `seen`, and the `Seen` to keep for next time.
///
/// A machine seen for the first time (first run, newly linked) sets the
/// baseline without alerting. Drift is reported for every machine, so the
/// studio hears when the laptop slips; suggestions only for this machine,
/// where the user just installed something. Machines no longer present are
/// forgotten.
pub fn alerts(seen: &Seen, now: &[Observed]) -> (Vec<Alert>, Seen) {
    let mut out = Vec::new();
    let mut next = Seen::default();
    for m in now {
        let entry = SeenMachine {
            gaps: m.gaps.iter().flat_map(|(k, _)| k.iter().cloned()).collect(),
            suggestions: m.suggestions.iter().map(|(k, _)| k.clone()).collect(),
        };
        if let Some(before) = seen.machines.get(&m.machine_id) {
            let items: Vec<String> = m
                .gaps
                .iter()
                .filter(|(k, _)| k.iter().any(|k| !before.gaps.contains(k)))
                .map(|(_, s)| s.clone())
                .collect();
            let items = dedup(items);
            if !items.is_empty() {
                out.push(Alert::Drift {
                    machine_id: m.machine_id.clone(),
                    machine: m.machine_name.clone(),
                    score: m.score,
                    items,
                });
            }
            let products: Vec<String> = m
                .suggestions
                .iter()
                .filter(|(k, _)| !before.suggestions.contains(k))
                .map(|(_, n)| n.clone())
                .collect();
            let products = dedup(products);
            if m.is_this_machine && !products.is_empty() {
                out.push(Alert::Suggest {
                    machine_id: m.machine_id.clone(),
                    machine: m.machine_name.clone(),
                    products,
                });
            }
        }
        next.machines.insert(m.machine_id.clone(), entry);
    }
    (out, next)
}

/// Drop repeats, keeping the first of each.
fn dedup(v: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    v.into_iter().filter(|s| seen.insert(s.clone())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compare;
    use patchpilot_core::{Item, Manifest, Product, ProductId, TargetProfile};

    fn load(rel: &str) -> Manifest {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures")
            .join(rel);
        Manifest::from_json(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    fn golden() -> Manifest {
        load("parity/score/golden.json")
    }

    /// The studio's own scan with one change applied.
    fn studio(change: impl FnOnce(&mut Manifest)) -> Manifest {
        let mut m = golden();
        m.machine.id = "studio".into();
        change(&mut m);
        m
    }

    fn observe(m: &Manifest, this: bool, ignored: &BTreeSet<String>) -> Observed {
        let c = compare(&golden(), &TargetProfile::default(), m);
        Observed::new(&m.machine.id, &m.machine.name, this, &c, ignored)
    }

    fn add_product(m: &mut Manifest, id: &str, name: &str, version: &str) {
        let mut item: Item = m.items[0].clone();
        item.product_id = Some(ProductId(id.into()));
        item.version = Some(version.into());
        m.items.push(item);
        let mut p: Product = m.products[0].clone();
        p.canonical_id = ProductId(id.into());
        p.name = name.into();
        m.products.push(p);
    }

    fn bump(m: &mut Manifest, id: &str, version: &str) {
        for i in &mut m.items {
            if i.product_id.as_ref().is_some_and(|p| p.0 == id) {
                i.version = Some(version.into());
            }
        }
    }

    #[test]
    fn extras_and_newer_versions_are_suggestions_folders_are_not() {
        let m = studio(|m| {
            add_product(m, "acme-delay", "Acme Delay", "1.0.0");
            bump(m, "acme-comp", "3.2.0");
            m.items.retain(|i| i.kind != ItemKind::Folder);
        });
        let c = compare(&golden(), &TargetProfile::default(), &m);
        let names: Vec<_> = suggestions(&c).map(|r| r.name.as_str()).collect();
        assert_eq!(names.len(), 2, "{names:?}");
        assert!(names.contains(&"Acme Delay") && names.contains(&"Acme Comp"));
        let gaps: Vec<_> = gaps(&c).map(|r| r.name.as_str()).collect();
        assert_eq!(gaps, ["Big Strings Library"]);
    }

    #[test]
    fn first_sight_is_a_baseline_then_only_new_things_alert() {
        let none = BTreeSet::new();
        let laptop = load("parity/score/laptop.json");
        let (a, seen) = alerts(&Seen::default(), &[observe(&laptop, false, &none)]);
        assert!(a.is_empty(), "{a:?}");

        // Nothing changed: quiet.
        let (a, seen) = alerts(&seen, &[observe(&laptop, false, &none)]);
        assert!(a.is_empty());

        // The laptop loses Acme Verb.
        let mut worse = laptop.clone();
        worse
            .items
            .retain(|i| i.product_id.as_ref().is_none_or(|p| p.0 != "acme-verb"));
        let (a, seen) = alerts(&seen, &[observe(&worse, false, &none)]);
        let [Alert::Drift { items, score, .. }] = a.as_slice() else {
            panic!("{a:?}")
        };
        assert_eq!(items, &["Acme Verb is missing"]);
        assert!(*score < 100);
        assert_eq!(a[0].title(), "Travel Laptop drifted from the Primary Rig");
        assert!(a[0].body().starts_with("Acme Verb is missing. Parity "));

        // Same gap again: not repeated.
        let (a, _) = alerts(&seen, &[observe(&worse, false, &none)]);
        assert!(a.is_empty());
    }

    #[test]
    fn a_new_plugin_here_is_suggested_once_and_ignoring_silences_it() {
        let none = BTreeSet::new();
        let before = studio(|_| {});
        let after = studio(|m| add_product(m, "acme-delay", "Acme Delay", "1.0.0"));
        let (_, seen) = alerts(&Seen::default(), &[observe(&before, true, &none)]);

        // On another machine it's only shown in the app, not as an alert.
        let (a, _) = alerts(&seen, &[observe(&after, false, &none)]);
        assert!(a.is_empty());

        let (a, next) = alerts(&seen, &[observe(&after, true, &none)]);
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].title(), "New on Studio PC: Acme Delay");
        assert!(a[0].body().starts_with("It isn't in the Primary Rig yet"));
        let (a, _) = alerts(&next, &[observe(&after, true, &none)]);
        assert!(a.is_empty());

        let ignored = BTreeSet::from([suggestion_key("acme-delay", Some("1.0.0"))]);
        let o = observe(&after, true, &ignored);
        assert!(o.suggestions.is_empty());
        let (a, _) = alerts(&seen, &[o]);
        assert!(a.is_empty());
    }

    #[test]
    fn reapack_scripts_alert_as_one_thing() {
        let none = BTreeSet::new();
        let before = studio(|_| {});
        let after = studio(|m| {
            for n in 0..50 {
                add_product(
                    m,
                    &format!("reapack/reateam/various/s{n}"),
                    &format!("Script {n}"),
                    "1.0",
                );
            }
            add_product(m, "acme-delay", "Acme Delay", "1.0.0");
        });
        let (_, seen) = alerts(&Seen::default(), &[observe(&before, true, &none)]);
        let (a, _) = alerts(&seen, &[observe(&after, true, &none)]);
        let [Alert::Suggest { products, .. }] = a.as_slice() else {
            panic!("{a:?}")
        };
        assert_eq!(products, &["Acme Delay", "ReaPack packages"]);
    }

    #[test]
    fn lists_are_short() {
        let items: Vec<String> = ["a is missing", "b", "c", "d"].map(String::from).into();
        assert_eq!(sentence_list(&items), "A is missing, b and 2 more");
        assert_eq!(sentence_list(&items[..1]), "A is missing");
    }

    #[test]
    fn seen_round_trips_as_json() {
        let laptop = load("parity/score/laptop.json");
        let (_, seen) = alerts(
            &Seen::default(),
            &[observe(&laptop, false, &BTreeSet::new())],
        );
        let json = serde_json::to_string(&seen).unwrap();
        assert_eq!(serde_json::from_str::<Seen>(&json).unwrap(), seen);
        assert_eq!(serde_json::from_str::<Seen>("{}").unwrap(), Seen::default());
    }

    #[test]
    fn script_packages_count_once_and_suggestions_add_up_by_group() {
        let m = studio(|m| {
            add_product(m, "acme-delay", "Acme Delay", "1.0.0");
            for (id, name) in [
                (
                    "reaper-script/a",
                    "Scripts/ReaTeam Scripts/Items Editing/a.lua",
                ),
                (
                    "reaper-script/b",
                    "Scripts/ReaTeam Scripts/Items Editing/b.lua",
                ),
                ("reaper-script/c", "Scripts/c.lua"),
                ("reaper-template/t", "TrackTemplates/Vocals.RTrackTemplate"),
                ("reapack/one", "Script: One.lua"),
                ("reapack/two", "Script: Two.lua"),
            ] {
                add_product(m, id, name, "1");
            }
        });
        let c = compare(&golden(), &TargetProfile::default(), &m);
        assert_eq!(
            suggestion_summary(suggestions(&c), &Packages::default()),
            [
                (SuggestionGroup::Plugins, 1),
                (SuggestionGroup::Scripts, 3),
                (SuggestionGroup::Templates, 1),
            ]
        );
        let names = observe(&m, true, &BTreeSet::new()).suggestions;
        assert!(names.iter().any(|(_, n)| n == "Items Editing"), "{names:?}");
    }

    #[test]
    fn files_linked_across_folders_count_once() {
        let files = [
            ("reaper-script/tool", "Scripts/Tools/tool.lua"),
            ("reaper-script/lib", "Scripts/Lib/core.lua"),
        ];
        let m = studio(|m| {
            for (id, rel) in files {
                add_product(m, id, rel, "1");
                let item = m.items.last_mut().unwrap();
                item.path =
                    patchpilot_core::PortablePath::logical(crate::reaper::REAPER_RESOURCES, rel);
                let other = files.iter().find(|(_, r)| *r != rel).unwrap().1;
                item.needs_files = vec![other.to_owned()];
            }
        });
        let c = compare(&golden(), &TargetProfile::default(), &m);
        let scripts = |p: &Packages| suggestion_summary(suggestions(&c), p);
        assert_eq!(
            scripts(&Packages::default()),
            [(SuggestionGroup::Scripts, 2)]
        );
        assert_eq!(scripts(&Packages::of(&m)), [(SuggestionGroup::Scripts, 1)]);
    }
}
