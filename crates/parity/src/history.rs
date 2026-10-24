//! What changed between two scans of a machine, for History: plug-ins that
//! appeared, disappeared or changed version. Going back to an older scan
//! reads the same list the other way round: reinstall what's gone, remove
//! what's new.

use std::collections::BTreeMap;

use patchpilot_core::Manifest;
use serde::Serialize;

use crate::inventory::{ProductRow, inventory};
use crate::{VersionOrder, compare_versions};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Change {
    Added,
    Removed,
    Updated,
    Downgraded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProductChange {
    pub product_id: String,
    pub name: String,
    pub vendor: String,
    pub change: Change,
    /// Version in `old` (`None` when added or unreadable).
    pub from: Option<String>,
    /// Version in `new` (`None` when removed or unreadable).
    pub to: Option<String>,
}

/// Plug-in products that differ from `old` to `new`, sorted by vendor then
/// name. A version change counts only when both versions could be read.
pub fn plugin_changes(old: &Manifest, new: &Manifest) -> Vec<ProductChange> {
    let before = by_id(old);
    let after = by_id(new);
    let mut out = Vec::new();
    for (id, (vendor, p)) in &after {
        let change = match before.get(id) {
            None => Some(Change::Added),
            Some((_, b)) => match (&b.version, &p.version) {
                (Some(f), Some(t)) => match compare_versions(f, t) {
                    VersionOrder::Older => Some(Change::Updated),
                    VersionOrder::Newer => Some(Change::Downgraded),
                    _ => None,
                },
                _ => None,
            },
        };
        if let Some(change) = change {
            out.push(ProductChange {
                product_id: (*id).to_owned(),
                name: p.name.clone(),
                vendor: vendor.clone(),
                change,
                from: before.get(id).and_then(|(_, b)| b.version.clone()),
                to: p.version.clone(),
            });
        }
    }
    for (id, (vendor, b)) in &before {
        if !after.contains_key(id) {
            out.push(ProductChange {
                product_id: (*id).to_owned(),
                name: b.name.clone(),
                vendor: vendor.clone(),
                change: Change::Removed,
                from: b.version.clone(),
                to: None,
            });
        }
    }
    out.sort_by(|a, b| (&a.vendor, &a.name).cmp(&(&b.vendor, &b.name)));
    out
}

fn by_id(m: &Manifest) -> BTreeMap<String, (String, ProductRow)> {
    inventory(m)
        .vendors
        .into_iter()
        .flat_map(|v| {
            let vendor = v.vendor;
            v.products
                .into_iter()
                .map(move |p| (p.id.clone(), (vendor.clone(), p)))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Manifest {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/core/manifest-v1/studio-pc.json");
        Manifest::from_json(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn lists_added_removed_and_version_changes() {
        let old = fixture();
        assert!(plugin_changes(&old, &old).is_empty());

        let mut new = old.clone();
        let plugin = |m: &Manifest| {
            m.items
                .iter()
                .position(|i| i.kind == patchpilot_core::ItemKind::Plugin && i.version.is_some())
                .unwrap()
        };
        let i = plugin(&new);
        let id = new.items[i].product_id.clone().unwrap();
        // Drop every item of that product: it was removed.
        new.items.retain(|it| it.product_id.as_ref() != Some(&id));
        let changes = plugin_changes(&old, &new);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].change, Change::Removed);
        assert_eq!(changes[0].product_id, id.0);
        // The other way round it was added.
        assert_eq!(plugin_changes(&new, &old)[0].change, Change::Added);

        let mut bumped = old.clone();
        for it in bumped
            .items
            .iter_mut()
            .filter(|it| it.product_id.as_ref() == Some(&id))
        {
            it.version = Some("999.0".into());
        }
        let changes = plugin_changes(&old, &bumped);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].change, Change::Updated);
        assert_eq!(changes[0].to.as_deref(), Some("999.0"));
        assert_eq!(plugin_changes(&bumped, &old)[0].change, Change::Downgraded);
    }
}
