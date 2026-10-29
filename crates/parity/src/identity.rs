//! Identity resolution between manifests.
//!
//! v1 product IDs are `<vendor>/<name>` slugs (see `patchpilot-scanner`).
//! When a scanner couldn't read a vendor (e.g. a VST3 with only an
//! `Info.plist`), the ID is `unknown/<name>`. [`align_products`] renames those
//! to the one known `<vendor>/<name>` with the same name across both
//! manifests, if there is exactly one.
//!
//! First, both manifests go through the plug-in catalog
//! ([`patchpilot_catalog::Catalog::canonicalize`]), which knows vendors'
//! other names and products' other spellings. Scans are canonicalized when
//! they are taken; doing it again here covers scans and Primary Rig versions saved
//! by older builds or against an older catalog.

use std::collections::{BTreeMap, HashSet};

use patchpilot_core::{Manifest, ProductId};

const UNKNOWN: &str = "unknown/";

/// Canonicalize both manifests through the catalog, then align
/// `unknown/<name>` product IDs between them. Returns how many distinct IDs
/// were renamed.
pub fn align_products(a: &mut Manifest, b: &mut Manifest) -> usize {
    let catalog = patchpilot_catalog::current();
    let canonical = catalog.canonicalize(a) + catalog.canonicalize(b);
    // name slug -> known IDs with that name
    let mut known: BTreeMap<String, Vec<ProductId>> = BTreeMap::new();
    for p in a.products.iter().chain(&b.products) {
        let id = &p.canonical_id.0;
        if id.starts_with(UNKNOWN) {
            continue;
        }
        if let Some((_, name)) = id.split_once('/') {
            let ids = known.entry(name.to_owned()).or_default();
            if !ids.contains(&p.canonical_id) {
                ids.push(p.canonical_id.clone());
            }
        }
    }
    // Unknown IDs come from products and from what templates require.
    let unknown: Vec<ProductId> = a
        .products
        .iter()
        .chain(&b.products)
        .map(|p| &p.canonical_id)
        .chain(a.items.iter().chain(&b.items).flat_map(|i| &i.requires))
        .filter(|id| id.0.starts_with(UNKNOWN))
        .cloned()
        .collect();
    let renames: BTreeMap<ProductId, ProductId> = unknown
        .into_iter()
        .filter_map(|id| {
            let name = id.0.strip_prefix(UNKNOWN)?;
            match known.get(name).map(Vec::as_slice) {
                Some([only]) => Some((id.clone(), only.clone())),
                _ => None,
            }
        })
        .collect();
    for m in [a, b] {
        rename(m, &renames);
    }
    canonical + renames.len()
}

fn rename(m: &mut Manifest, renames: &BTreeMap<ProductId, ProductId>) {
    for item in &mut m.items {
        if let Some(new) = item.product_id.as_ref().and_then(|id| renames.get(id)) {
            item.product_id = Some(new.clone());
        }
        if item.requires.iter().any(|id| renames.contains_key(id)) {
            for id in &mut item.requires {
                if let Some(new) = renames.get(id) {
                    *id = new.clone();
                }
            }
            item.requires.sort();
            item.requires.dedup();
        }
    }
    let mut products = std::mem::take(&mut m.products);
    for p in &mut products {
        if let Some(new) = renames.get(&p.canonical_id) {
            p.canonical_id = new.clone();
        }
    }
    // Drop duplicates created by the rename, keeping the entry that has a vendor.
    products.sort_by_key(|p| p.vendor.is_empty());
    let mut seen = HashSet::new();
    for p in products {
        if seen.insert(p.canonical_id.clone()) {
            m.products.push(p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patchpilot_core::{
        Arch, Item, MachineInfo, Os, PathMap, PluginFormat, PortablePath, Product,
    };

    fn manifest(os: Os, products: &[(&str, &str)]) -> Manifest {
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
        for (id, vendor) in products {
            m.products.push(Product {
                canonical_id: (*id).into(),
                vendor: (*vendor).into(),
                name: id.split('/').nth(1).unwrap().into(),
            });
            let mut item = Item::plugin(
                PluginFormat::Vst3,
                os,
                PortablePath::logical("VST3_SYSTEM", *id),
            );
            item.product_id = Some((*id).into());
            m.items.push(item);
        }
        m
    }

    #[test]
    fn unknown_vendor_takes_the_unique_known_one() {
        let mut win = manifest(Os::Windows, &[("acme-audio/acme-delay", "Acme Audio")]);
        let mut mac = manifest(
            Os::Macos,
            &[("unknown/acme-delay", ""), ("unknown/solo", "")],
        );
        assert_eq!(align_products(&mut win, &mut mac), 1);
        assert_eq!(
            mac.items[0].product_id.as_ref().unwrap().0,
            "acme-audio/acme-delay"
        );
        assert_eq!(mac.items[1].product_id.as_ref().unwrap().0, "unknown/solo");
        assert_eq!(mac.products.len(), 2);
    }

    #[test]
    fn ambiguous_names_are_left_alone() {
        let mut a = manifest(
            Os::Windows,
            &[("acme/comp", "Acme"), ("other/comp", "Other")],
        );
        let mut b = manifest(Os::Macos, &[("unknown/comp", "")]);
        assert_eq!(align_products(&mut a, &mut b), 0);
        assert_eq!(b.items[0].product_id.as_ref().unwrap().0, "unknown/comp");
    }
}
