//! Update alerts: plug-ins on a machine that have a newer version somewhere.
//!
//! "Somewhere" is another of the user's machines, the Primary Rig, or the
//! plug-in catalog's latest release. Only versions that parse and compare as
//! older raise an alert; an unreadable version never does (decision #17).
//! Formats are compared like for like where both sides have them, and a
//! version that only leaves off detail isn't older (decision #88).
//! The alert names where the newer version was seen, so the user can judge it.

use std::collections::BTreeMap;

use patchpilot_catalog::Catalog;
use patchpilot_core::{Item, ItemKind, Manifest, PluginFormat, ProductId};
use serde::Serialize;

use crate::version::{compare_by_format, newest, versions_by_format};
use crate::{VersionOrder, compare_releases, compare_versions};

/// Where a newer version was seen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UpdateSource {
    /// Another machine's latest scan.
    Machine { name: String },
    /// The latest Primary Rig version.
    Golden { version: u32 },
    /// The plug-in catalog.
    Catalog { released: Option<String> },
}

/// A manifest to look for newer versions in, and what to call it.
#[derive(Debug, Clone, Copy)]
pub struct Elsewhere<'a> {
    pub source: &'a UpdateSource,
    pub manifest: &'a Manifest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UpdateAlert {
    pub id: String,
    pub vendor: String,
    pub name: String,
    pub formats: Vec<PluginFormat>,
    /// The newest version installed here.
    pub installed: String,
    pub latest: String,
    pub source: UpdateSource,
    /// The vendor's website, from the catalog.
    pub website: Option<String>,
}

/// Plug-ins on `machine` with a newer version in `elsewhere` or `catalog`,
/// sorted by vendor and name. Every manifest is canonicalized through the
/// catalog first, so different spellings of one product compare.
pub fn update_alerts(
    machine: &Manifest,
    elsewhere: &[Elsewhere<'_>],
    catalog: &Catalog,
) -> Vec<UpdateAlert> {
    let mut here = machine.clone();
    catalog.canonicalize(&mut here);
    let installed = plugins_by_product(&here);
    let others: Vec<(&UpdateSource, Manifest)> = elsewhere
        .iter()
        .map(|e| {
            let mut m = e.manifest.clone();
            catalog.canonicalize(&mut m);
            (e.source, m)
        })
        .collect();
    let seen: Vec<(&UpdateSource, Plugins)> = others
        .iter()
        .map(|(s, m)| (*s, plugins_by_product(m)))
        .collect();

    // Per product: the installed version that's behind, the newest version
    // it's behind, and where that was seen.
    let mut best: BTreeMap<&ProductId, (String, String, UpdateSource)> = BTreeMap::new();
    let mut offer = |id, ours: &str, latest: &str, source: &UpdateSource| {
        let newer = best
            .get(id)
            .is_none_or(|(_, cur, _)| compare_versions(cur, latest) == VersionOrder::Older);
        if newer {
            best.insert(id, (ours.to_owned(), latest.to_owned(), source.clone()));
        }
    };
    for (id, ours) in &installed {
        for (source, plugins) in &seen {
            if let Some(theirs) = plugins.get(id)
                && let Some((VersionOrder::Older, o, t)) = compare_by_format(ours, theirs)
            {
                offer(id, o, t, source);
            }
        }
        if let Some(release) = catalog.product(id).and_then(|p| p.latest.as_ref())
            && let Some(o) = newest(ours)
            && compare_releases(o, &release.version) == VersionOrder::Older
        {
            offer(
                id,
                o,
                &release.version,
                &UpdateSource::Catalog {
                    released: release.released.clone(),
                },
            );
        }
    }

    let mut alerts: Vec<UpdateAlert> = best
        .into_iter()
        .map(|(id, (version, latest, source))| {
            let product = here.product(id);
            let vendor_id = id.0.split_once('/').map_or("", |(v, _)| v);
            UpdateAlert {
                id: id.0.clone(),
                vendor: product.map(|p| p.vendor.clone()).unwrap_or_default(),
                name: product.map_or_else(|| id.0.clone(), |p| p.name.clone()),
                formats: installed[id].iter().filter_map(|(f, _)| *f).collect(),
                installed: version,
                latest,
                source,
                website: catalog.vendor(vendor_id).and_then(|v| v.website.clone()),
            }
        })
        .collect();
    alerts.sort_by(|a, b| {
        (a.vendor.to_lowercase(), a.name.to_lowercase())
            .cmp(&(b.vendor.to_lowercase(), b.name.to_lowercase()))
    });
    alerts
}

/// Plug-in product -> the newest readable version of each of its formats.
type Plugins<'a> = BTreeMap<&'a ProductId, Vec<(Option<PluginFormat>, &'a str)>>;

fn plugins_by_product(m: &Manifest) -> Plugins<'_> {
    let mut items: BTreeMap<&ProductId, Vec<&Item>> = BTreeMap::new();
    for item in m.items.iter().filter(|i| i.kind == ItemKind::Plugin) {
        if let Some(id) = &item.product_id {
            items.entry(id).or_default().push(item);
        }
    }
    items
        .into_iter()
        .map(|(id, items)| (id, versions_by_format(items)))
        .filter(|(_, v)| !v.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use patchpilot_catalog::CatalogFile;
    use patchpilot_core::{Arch, Item, MachineInfo, Os, PathMap, PortablePath, Product};

    fn manifest(os: Os, plugins: &[(&str, &str, &str, PluginFormat)]) -> Manifest {
        let mut m = Manifest::new(
            MachineInfo {
                id: "m".into(),
                name: "m".into(),
                os,
                arch: Arch::X86_64,
                os_version: None,
                path_map: PathMap::new(os),
                disks: Vec::new(),
            },
            0,
        );
        for (id, vendor, version, format) in plugins {
            if m.product(&(*id).into()).is_none() {
                m.products.push(Product {
                    canonical_id: (*id).into(),
                    vendor: (*vendor).into(),
                    name: id.split('/').nth(1).unwrap().into(),
                });
            }
            let mut item = Item::plugin(*format, os, PortablePath::logical("VST3_SYSTEM", *id));
            item.product_id = Some((*id).into());
            item.version = Some((*version).into());
            m.items.push(item);
        }
        m
    }

    #[test]
    fn newer_on_another_machine_raises_an_alert() {
        let catalog = Catalog::bundled();
        let pc = manifest(
            Os::Windows,
            &[
                (
                    "fabfilter/fabfilter-pro-q-4",
                    "FabFilter",
                    "4.0.1",
                    PluginFormat::Vst3,
                ),
                ("acme/verb", "Acme", "2.0", PluginFormat::Vst3),
                ("acme/odd", "Acme", "nightly", PluginFormat::Vst3),
            ],
        );
        let mac = manifest(
            Os::Macos,
            &[
                (
                    "fabfilter-software-instruments/pro-q-4",
                    "FabFilter Software Instruments",
                    "4.0.2",
                    PluginFormat::Au,
                ),
                ("acme/verb", "Acme", "1.9", PluginFormat::Au),
                ("acme/odd", "Acme", "3.0", PluginFormat::Au),
            ],
        );
        let source = UpdateSource::Machine {
            name: "Studio Mac".into(),
        };
        let alerts = update_alerts(
            &pc,
            &[Elsewhere {
                source: &source,
                manifest: &mac,
            }],
            &catalog,
        );
        assert_eq!(alerts.len(), 1, "{alerts:#?}");
        let a = &alerts[0];
        assert_eq!(a.id, "fabfilter/pro-q-4");
        assert_eq!(
            (a.vendor.as_str(), a.name.as_str()),
            ("FabFilter", "Pro-Q 4")
        );
        assert_eq!(
            (a.installed.as_str(), a.latest.as_str()),
            ("4.0.1", "4.0.2")
        );
        assert_eq!(a.source, source);
        assert_eq!(a.website.as_deref(), Some("https://www.fabfilter.com"));
    }

    #[test]
    fn catalog_latest_raises_an_alert_and_the_newest_source_wins() {
        let mut file: CatalogFile = Catalog::bundled().file().clone();
        for p in &mut file.products {
            if p.name == "Diva" {
                p.latest = Some(patchpilot_catalog::Release {
                    version: "1.5.0".into(),
                    released: Some("2026-09-01".into()),
                });
            }
        }
        let catalog = Catalog::new(file).unwrap();
        let here = manifest(
            Os::Macos,
            &[("u-he/diva", "u-he", "1.4.8", PluginFormat::Vst3)],
        );
        let golden = manifest(
            Os::Macos,
            &[("u-he/diva", "u-he", "1.4.9", PluginFormat::Vst3)],
        );
        let g = UpdateSource::Golden { version: 3 };
        let alerts = update_alerts(
            &here,
            &[Elsewhere {
                source: &g,
                manifest: &golden,
            }],
            &catalog,
        );
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].latest, "1.5.0");
        assert_eq!(
            alerts[0].source,
            UpdateSource::Catalog {
                released: Some("2026-09-01".into())
            }
        );
    }

    #[test]
    fn formats_compare_like_for_like_and_build_numbers_add_detail_only() {
        let catalog = Catalog::bundled();
        let pc = manifest(
            Os::Windows,
            &[
                ("arturia/acid-v", "Arturia", "1.1", PluginFormat::Vst3),
                (
                    "xfer-records/serum",
                    "Xfer Records",
                    "1,3,6,8",
                    PluginFormat::Vst3,
                ),
                ("acme/old", "Acme", "2.0", PluginFormat::Vst2),
                ("acme/old", "Acme", "2.4", PluginFormat::Vst3),
            ],
        );
        let mac = manifest(
            Os::Macos,
            &[
                (
                    "arturia/acid-v",
                    "Arturia",
                    "1.1.5.6367",
                    PluginFormat::Vst3,
                ),
                // Arturia's AAX wrapper carries its own number.
                ("arturia/acid-v", "Arturia", "1.4.6", PluginFormat::Aax),
                (
                    "xfer-records/serum",
                    "Xfer Records",
                    "1.368",
                    PluginFormat::Vst3,
                ),
                ("acme/old", "Acme", "2.4", PluginFormat::Vst2),
            ],
        );
        let source = UpdateSource::Machine { name: "Mac".into() };
        let alerts = update_alerts(
            &pc,
            &[Elsewhere {
                source: &source,
                manifest: &mac,
            }],
            &catalog,
        );
        let got: Vec<(&str, &str, &str)> = alerts
            .iter()
            .map(|a| (a.id.as_str(), a.installed.as_str(), a.latest.as_str()))
            .collect();
        // Only the old VST2 copy is behind.
        assert_eq!(got, [("acme/old", "2.0", "2.4")]);
    }
}
