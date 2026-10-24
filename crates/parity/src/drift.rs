//! Drift alerts and suggestions for the Golden Rig (M9, strategy points 3 and 7).
//!
//! A **gap** is something the Golden Rig has that a machine lacks or has an
//! older version of. A **suggestion** is the other way round: a product the
//! machine has that Golden lacks, or has an older version of, which the user
//! may want to add to Golden ("propose to Golden").
//!
//! [`alerts`] compares what each machine looks like now with what the user
//! was last told about ([`Seen`]) and returns only what is new, so an alert
//! never repeats. Pure: the caller stores `Seen` and shows the alerts.

use std::collections::{BTreeMap, BTreeSet};

use patchpilot_core::ItemKind;
use serde::{Deserialize, Serialize};

use crate::{Comparison, ComparisonRow, ParityState};

/// Rows of `c` that could go into the Golden Rig: products Golden doesn't
/// have, or has an older version of. Folders and configs are left out (a
/// sample library or a settings file isn't something to add from one
/// machine).
pub fn suggestions(c: &Comparison) -> impl Iterator<Item = &ComparisonRow> {
    c.rows.iter().filter(|r| {
        matches!(r.state, ParityState::Extra | ParityState::NewerThanGolden)
            && !matches!(r.kind, ItemKind::Folder | ItemKind::Config)
    })
}

/// Identifies a suggestion across scans and machines: the product and the
/// machine's version, so ignoring Serum 2.0 still suggests Serum 2.1 later.
pub fn suggestion_key(product_id: &str, version: Option<&str>) -> String {
    format!("{product_id}@{}", version.unwrap_or_default())
}

fn row_suggestion_key(r: &ComparisonRow) -> String {
    suggestion_key(&r.product_id.0, r.machine_version.as_deref())
}

/// Rows of `c` where the machine falls short of Golden.
pub fn gaps(c: &Comparison) -> impl Iterator<Item = &ComparisonRow> {
    c.rows.iter().filter(|r| {
        matches!(
            r.state,
            ParityState::Missing | ParityState::Incomplete | ParityState::Outdated
        )
    })
}

/// A gap is new when the product is short of a Golden version it wasn't short
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
                .map(|r| (row_suggestion_key(r), r.name.clone()))
                .filter(|(k, _)| !ignored.contains(k))
                .collect(),
        }
    }
}

fn gap_sentence(r: &ComparisonRow) -> String {
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
    /// A machine fell further from the Golden Rig.
    Drift {
        machine_id: String,
        machine: String,
        score: u8,
        /// New gaps, as sentences.
        items: Vec<String>,
    },
    /// This machine has something new the Golden Rig doesn't.
    Suggest {
        machine_id: String,
        machine: String,
        products: Vec<String>,
    },
}

impl Alert {
    pub fn title(&self) -> String {
        match self {
            Self::Drift { machine, .. } => format!("{machine} drifted from the Golden Rig"),
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
                format!("{list}{what} in the Golden Rig yet. Open Compare to add or ignore.")
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
        assert_eq!(a[0].title(), "Travel Laptop drifted from the Golden Rig");
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
        assert!(a[0].body().starts_with("It isn't in the Golden Rig yet"));
        let (a, _) = alerts(&next, &[observe(&after, true, &none)]);
        assert!(a.is_empty());

        let ignored = BTreeSet::from([suggestion_key("acme-delay", Some("1.0.0"))]);
        let o = observe(&after, true, &ignored);
        assert!(o.suggestions.is_empty());
        let (a, _) = alerts(&seen, &[o]);
        assert!(a.is_empty());
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
}
