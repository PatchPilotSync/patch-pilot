//! "Here's your rig" (#40): one machine's manifest as counts and products
//! grouped by vendor, for the first-scan screen.

use std::collections::BTreeMap;

use patchpilot_core::{Confidence, ItemKind, Manifest, PluginFormat, PortablePath};
use serde::Serialize;

use crate::comparison::confidence_rank;
use crate::{VersionOrder, compare_versions};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Inventory {
    /// Plug-in items (one per format and file).
    pub plugins: usize,
    /// Distinct plug-in products.
    pub products: usize,
    /// Plug-in items per format, in [`PluginFormat`] order, zero counts left out.
    pub formats: Vec<FormatCount>,
    /// Plug-in products by vendor, sorted by vendor name; products without a
    /// known vendor come last under an empty `vendor`.
    pub vendors: Vec<VendorGroup>,
    /// Vendor apps and licensing tools, by name.
    pub apps: Vec<AppRow>,
    /// DAW templates, project templates and FX chains.
    pub templates: usize,
    /// Third-party preset folders found ([`crate::presets`]), by name.
    pub presets: Vec<PresetRow>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PresetRow {
    pub name: String,
    pub vendor: String,
    /// Bytes in the folder.
    pub size: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FormatCount {
    pub format: PluginFormat,
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VendorGroup {
    pub vendor: String,
    pub products: Vec<ProductRow>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProductRow {
    pub id: String,
    pub name: String,
    pub formats: Vec<PluginFormat>,
    /// The newest version among its items, if any could be read.
    pub version: Option<String>,
    /// The weakest identity confidence among its items.
    pub confidence: Confidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AppRow {
    pub name: String,
    pub kind: ItemKind,
    pub version: Option<String>,
}

pub fn inventory(m: &Manifest) -> Inventory {
    let mut formats: BTreeMap<u8, FormatCount> = BTreeMap::new();
    // vendor (lowercased for grouping) -> product id -> row
    let mut vendors: BTreeMap<(bool, String), (String, BTreeMap<String, ProductRow>)> =
        BTreeMap::new();
    let mut apps = Vec::new();
    let mut plugins = 0;
    let mut templates = 0;
    let mut presets = Vec::new();

    for item in &m.items {
        let product = item.product_id.as_ref().and_then(|id| m.product(id));
        match item.kind {
            ItemKind::Plugin => {
                plugins += 1;
                let Some(format) = item.format else { continue };
                formats
                    .entry(format_rank(format))
                    .or_insert(FormatCount { format, count: 0 })
                    .count += 1;
                let (id, name, vendor) = match product {
                    Some(p) => (p.canonical_id.0.clone(), p.name.clone(), p.vendor.clone()),
                    None => (
                        path_label(&item.path),
                        path_label(&item.path),
                        String::new(),
                    ),
                };
                let vendor = vendor.trim().to_owned();
                let group = vendors
                    .entry((vendor.is_empty(), vendor.to_lowercase()))
                    .or_insert_with(|| (vendor.clone(), BTreeMap::new()));
                let row = group.1.entry(id.clone()).or_insert_with(|| ProductRow {
                    id,
                    name,
                    formats: Vec::new(),
                    version: None,
                    confidence: item.confidence,
                });
                if !row.formats.contains(&format) {
                    row.formats.push(format);
                    row.formats.sort_by_key(|f| format_rank(*f));
                }
                if let Some(v) = &item.version {
                    let newer = row
                        .version
                        .as_deref()
                        .is_none_or(|cur| compare_versions(cur, v) == VersionOrder::Older);
                    if newer {
                        row.version = Some(v.clone());
                    }
                }
                row.confidence = weakest(row.confidence, item.confidence);
            }
            ItemKind::VendorApp | ItemKind::LicenseTool => apps.push(AppRow {
                name: product.map_or_else(|| path_label(&item.path), |p| p.name.clone()),
                kind: item.kind,
                version: item.version.clone(),
            }),
            ItemKind::Folder if crate::presets::is_preset_item(item) => {
                presets.push(PresetRow {
                    name: product.map_or_else(|| path_label(&item.path), |p| p.name.clone()),
                    vendor: product.map(|p| p.vendor.clone()).unwrap_or_default(),
                    size: item.size,
                });
            }
            ItemKind::DawAsset if is_template(item) => {
                templates += 1;
            }
            _ => {}
        }
    }

    apps.sort_by_key(|a| a.name.to_lowercase());
    presets.sort_by_key(|p| p.name.to_lowercase());
    let vendors: Vec<VendorGroup> = vendors
        .into_values()
        .map(|(vendor, products)| {
            let mut products: Vec<ProductRow> = products.into_values().collect();
            products.sort_by_key(|p| p.name.to_lowercase());
            VendorGroup { vendor, products }
        })
        .collect();
    Inventory {
        plugins,
        products: vendors.iter().map(|v| v.products.len()).sum(),
        formats: formats.into_values().collect(),
        vendors,
        apps,
        templates,
        presets,
    }
}

fn is_template(item: &patchpilot_core::Item) -> bool {
    item.product_id
        .as_ref()
        .is_some_and(|id| id.0.starts_with("reaper-template/"))
}

/// The file or folder name at the end of a path.
fn path_label(p: &PortablePath) -> String {
    let full = match p {
        PortablePath::Logical { rel, .. } => rel.as_str(),
        PortablePath::Absolute(a) => a.as_str(),
    };
    full.rsplit(['/', '\\'])
        .find(|s| !s.is_empty())
        .unwrap_or(full)
        .to_owned()
}

fn format_rank(f: PluginFormat) -> u8 {
    match f {
        PluginFormat::Vst3 => 0,
        PluginFormat::Au => 1,
        PluginFormat::Clap => 2,
        PluginFormat::Aax => 3,
        PluginFormat::Vst2 => 4,
        PluginFormat::Jsfx => 5,
    }
}

fn weakest(a: Confidence, b: Confidence) -> Confidence {
    if confidence_rank(a) <= confidence_rank(b) {
        a
    } else {
        b
    }
}
