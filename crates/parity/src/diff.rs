//! Diff a machine manifest against a Golden manifest, per product.
//!
//! Comparison is at the product level so a Windows VST3 and a macOS AU/VST3
//! of the same product count as the same thing. Items must already carry a
//! `product_id` (identity resolution); items without one are reported as
//! unresolved rather than guessed at.

use std::collections::BTreeMap;

use patchpilot_core::{Item, Manifest, Os, PluginFormat, ProductId};

use crate::version::{VersionOrder, compare_versions};

/// Where a product stands on the machine relative to Golden.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ParityState {
    /// In Golden, not on the machine.
    Missing,
    /// On the machine, but some formats Golden has are absent (e.g. no AAX).
    Incomplete,
    Outdated,
    /// A user-owned file whose content differs from Golden at the same version.
    Modified,
    /// Versions differ in a way we can't order (unparseable strings).
    VersionUnknown,
    NewerThanGolden,
    /// On the machine, not in Golden.
    Extra,
    /// Golden's formats of this product don't exist on this OS (e.g. AU on Windows).
    NotApplicable,
    InParity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductDiff {
    pub product_id: ProductId,
    /// Display name from whichever manifest knows the product.
    pub name: String,
    pub state: ParityState,
    pub golden_version: Option<String>,
    pub machine_version: Option<String>,
    /// Golden formats that apply to this OS but aren't installed.
    pub missing_formats: Vec<PluginFormat>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ManifestDiff {
    /// Sorted by state (most urgent first), then name.
    pub products: Vec<ProductDiff>,
    /// Machine items with no `product_id`, by index into `machine.items`.
    pub unresolved: Vec<usize>,
}

impl ManifestDiff {
    pub fn count(&self, state: ParityState) -> usize {
        self.products.iter().filter(|p| p.state == state).count()
    }
}

/// Whether a format can exist on an OS at all.
pub fn format_available_on(format: PluginFormat, os: Os) -> bool {
    !(format == PluginFormat::Au && os == Os::Windows)
}

pub fn diff_manifests(golden: &Manifest, machine: &Manifest) -> ManifestDiff {
    let os = machine.machine.os;
    let golden_by = by_product(golden);
    let machine_by = by_product(machine);

    let name_of = |id: &ProductId| {
        golden
            .product(id)
            .or_else(|| machine.product(id))
            .map_or_else(|| id.to_string(), |p| p.name.clone())
    };

    let mut products = Vec::new();
    for (&id, g_items) in &golden_by {
        let relevant = relevant_golden_items(g_items, os);
        let m_items = machine_by.get(id).map(Vec::as_slice).unwrap_or_default();
        let mut d = ProductDiff {
            product_id: id.clone(),
            name: name_of(id),
            state: ParityState::InParity,
            golden_version: max_version(&relevant),
            machine_version: max_version(m_items),
            missing_formats: Vec::new(),
        };
        if relevant.is_empty() {
            d.state = ParityState::NotApplicable;
        } else if m_items.is_empty() {
            d.state = ParityState::Missing;
        } else {
            d.state = compare_present(&relevant, m_items, &d);
            let have: Vec<_> = m_items.iter().filter_map(|i| i.format).collect();
            for f in relevant.iter().filter_map(|i| i.format) {
                if !have.contains(&f) && !d.missing_formats.contains(&f) {
                    d.missing_formats.push(f);
                }
            }
            if d.state == ParityState::InParity && !d.missing_formats.is_empty() {
                d.state = ParityState::Incomplete;
            }
        }
        products.push(d);
    }
    for (&id, m_items) in &machine_by {
        if !golden_by.contains_key(id) {
            products.push(ProductDiff {
                product_id: id.clone(),
                name: name_of(id),
                state: ParityState::Extra,
                golden_version: None,
                machine_version: max_version(m_items),
                missing_formats: Vec::new(),
            });
        }
    }
    products.sort_by(|a, b| (a.state, &a.name).cmp(&(b.state, &b.name)));

    let unresolved = machine
        .items
        .iter()
        .enumerate()
        .filter(|(_, i)| i.product_id.is_none())
        .map(|(n, _)| n)
        .collect();
    ManifestDiff {
        products,
        unresolved,
    }
}

fn by_product(m: &Manifest) -> BTreeMap<&ProductId, Vec<&Item>> {
    let mut by: BTreeMap<&ProductId, Vec<&Item>> = BTreeMap::new();
    for item in &m.items {
        if let Some(id) = &item.product_id {
            by.entry(id).or_default().push(item);
        }
    }
    by
}

/// Golden items that matter for a machine on `os`.
///
/// If Golden has items built for this OS (or portable ones), use those.
/// Otherwise Golden was scanned on the other OS: use its items whose format
/// exists here, since a VST3 on Windows implies a VST3 on macOS.
fn relevant_golden_items<'a>(items: &[&'a Item], os: Os) -> Vec<&'a Item> {
    let native: Vec<_> = items
        .iter()
        .copied()
        .filter(|i| i.platform == os || i.platform_portable)
        .collect();
    if !native.is_empty() {
        return native;
    }
    items
        .iter()
        .copied()
        .filter(|i| i.format.is_none_or(|f| format_available_on(f, os)))
        .collect()
}

fn compare_present(golden: &[&Item], machine: &[&Item], d: &ProductDiff) -> ParityState {
    if let (Some(g), Some(m)) = (&d.golden_version, &d.machine_version) {
        match compare_versions(m, g) {
            VersionOrder::Older => return ParityState::Outdated,
            VersionOrder::Newer => return ParityState::NewerThanGolden,
            VersionOrder::Unknown => return ParityState::VersionUnknown,
            VersionOrder::Same => {}
        }
    }
    // Same version (or none): user-owned files can still differ in content.
    let modified = machine.iter().any(|m| {
        m.content_hash.as_ref().is_some_and(|mh| {
            golden
                .iter()
                .filter(|g| g.format == m.format && g.path == m.path)
                .any(|g| g.content_hash.as_ref().is_some_and(|gh| gh != mh))
        })
    });
    if modified {
        ParityState::Modified
    } else {
        ParityState::InParity
    }
}

/// The highest version among items, by [`compare_versions`]. Unorderable
/// versions keep the first seen.
fn max_version(items: &[&Item]) -> Option<String> {
    items
        .iter()
        .filter_map(|i| i.version.as_deref())
        .fold(None, |best: Option<&str>, v| match best {
            Some(b) if compare_versions(v, b) != VersionOrder::Newer => Some(b),
            _ => Some(v),
        })
        .map(str::to_owned)
}
