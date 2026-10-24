//! The plug-in catalog: which vendor and product a plug-in really is, across
//! formats and OSes, and the newest version we know of.
//!
//! Product IDs from the scanners are name-based slugs (decision #20), so the
//! same plug-in can arrive as `fabfilter/fabfilter-pro-q-4` (a VST3 named
//! after its file), `fabfilter-software-instruments/pro-q-4` (an AU with the
//! full company name) or `unknown/pro-q-4` (no vendor readable). The catalog
//! knows each vendor's other names and each product's other spellings, and
//! [`Catalog::canonicalize`] rewrites a manifest to one ID per product.
//!
//! A copy ships inside the app (`data/catalog.json`). A newer one, fetched by
//! the app from the public releases repo (decision #43), replaces it with
//! [`install`]. Matching is deliberately conservative: an ID is only changed
//! when its vendor is known, or when an unknown-vendor name matches exactly
//! one catalog product.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, OnceLock, RwLock};

use patchpilot_core::{ItemKind, Manifest, ProductId};
use serde::{Deserialize, Serialize};

/// Version of the catalog JSON format this build reads.
pub const CATALOG_VERSION: u32 = 1;

const BUNDLED: &str = include_str!("../data/catalog.json");
const UNKNOWN: &str = "unknown";
/// A vendor name glued to a product name ("ValhallaVintageVerb") is only
/// stripped for vendor keys at least this long, so "NI" never eats "Nice".
const MIN_GLUED_PREFIX: usize = 6;

/// The catalog file, as written in `data/catalog.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogFile {
    pub catalog_version: u32,
    /// When the catalog was last edited, `YYYY-MM-DD`. A fetched catalog only
    /// replaces the bundled one when it is newer.
    pub updated: String,
    #[serde(default)]
    pub vendors: Vec<Vendor>,
    #[serde(default)]
    pub products: Vec<CatalogProduct>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Vendor {
    /// Slug used as the first half of product IDs, e.g. `fabfilter`.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Other names the vendor's plug-ins report (company names, short forms).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub website: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogProduct {
    /// A [`Vendor::id`].
    pub vendor: String,
    /// Display name, without the vendor in front unless the vendor brands it so.
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    /// The newest release we know of. Absent until someone has checked it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest: Option<Release>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Release {
    /// The vendor's version string, as their plug-ins report it.
    pub version: String,
    /// Release date, `YYYY-MM-DD`, if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub released: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    #[error(
        "catalog version {0} is newer than this build supports ({CATALOG_VERSION}); update Patch Pilot"
    )]
    UnsupportedVersion(u32),
    #[error("invalid catalog JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("catalog product {product:?} names unknown vendor {vendor:?}")]
    UnknownVendor { vendor: String, product: String },
}

/// What [`Catalog::resolve`] found for a product ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub id: ProductId,
    /// The vendor's display name.
    pub vendor: String,
    /// The catalog product, when the product itself (not just its vendor) is known.
    pub product: Option<CatalogProduct>,
    /// For a known vendor but unknown product: the product name with the
    /// vendor's name taken off the front, if it had one.
    stripped_name: bool,
}

/// A loaded catalog with its lookup tables.
#[derive(Debug)]
pub struct Catalog {
    file: CatalogFile,
    /// Compact vendor key (any of its names) -> index into `file.vendors`.
    vendor_by_key: HashMap<String, usize>,
    /// Vendor index -> its keys as slugs, longest first, for prefix stripping.
    vendor_slugs: Vec<Vec<String>>,
    /// (vendor index, compact product key) -> index into `file.products`.
    product_by_key: HashMap<(usize, String), usize>,
    /// Compact product key -> product indexes, for unknown-vendor IDs.
    product_by_name: HashMap<String, Vec<usize>>,
}

impl Catalog {
    /// The catalog compiled into this build.
    pub fn bundled() -> Self {
        Self::from_json(BUNDLED).expect("bundled catalog is valid")
    }

    pub fn from_json(json: &str) -> Result<Self, CatalogError> {
        #[derive(Deserialize)]
        struct Header {
            catalog_version: u32,
        }
        let Header { catalog_version } = serde_json::from_str(json)?;
        if catalog_version > CATALOG_VERSION {
            return Err(CatalogError::UnsupportedVersion(catalog_version));
        }
        Self::new(serde_json::from_str(json)?)
    }

    pub fn new(file: CatalogFile) -> Result<Self, CatalogError> {
        let mut vendor_by_key = HashMap::new();
        let mut vendor_slugs = Vec::new();
        for (i, v) in file.vendors.iter().enumerate() {
            let mut slugs: Vec<String> = [v.id.as_str(), v.name.as_str()]
                .into_iter()
                .chain(v.aliases.iter().map(String::as_str))
                .map(vendor_slug)
                .filter(|s| !s.is_empty())
                .collect();
            slugs.sort_by_key(|s| std::cmp::Reverse(s.len()));
            slugs.dedup();
            for s in &slugs {
                vendor_by_key.entry(compact(s)).or_insert(i);
            }
            vendor_slugs.push(slugs);
        }
        let mut catalog = Self {
            file,
            vendor_by_key,
            vendor_slugs,
            product_by_key: HashMap::new(),
            product_by_name: HashMap::new(),
        };
        for (i, p) in catalog.file.products.iter().enumerate() {
            let v = catalog
                .file
                .vendors
                .iter()
                .position(|v| v.id == p.vendor)
                .ok_or_else(|| CatalogError::UnknownVendor {
                    vendor: p.vendor.clone(),
                    product: p.name.clone(),
                })?;
            let names = std::iter::once(&p.name).chain(&p.aliases);
            for name in names {
                let key = compact(&catalog.strip_vendor(v, &slug(name)).0);
                catalog.product_by_key.entry((v, key.clone())).or_insert(i);
                let by_name = catalog.product_by_name.entry(key).or_default();
                if !by_name.contains(&i) {
                    by_name.push(i);
                }
            }
        }
        Ok(catalog)
    }

    pub fn file(&self) -> &CatalogFile {
        &self.file
    }

    /// `YYYY-MM-DD` of the last edit.
    pub fn updated(&self) -> &str {
        &self.file.updated
    }

    pub fn vendor(&self, id: &str) -> Option<&Vendor> {
        self.file.vendors.iter().find(|v| v.id == id)
    }

    /// The canonical ID of a catalog product.
    pub fn product_id(&self, p: &CatalogProduct) -> ProductId {
        ProductId(format!("{}/{}", p.vendor, slug(&p.name)))
    }

    /// The catalog product with this canonical ID, if any.
    pub fn product(&self, id: &ProductId) -> Option<&CatalogProduct> {
        self.file
            .products
            .iter()
            .find(|p| &self.product_id(p) == id)
    }

    /// Where this `<vendor>/<name>` product ID belongs, if the catalog knows
    /// its vendor (or, for `unknown/<name>`, exactly one product of that name).
    pub fn resolve(&self, id: &ProductId) -> Option<Resolved> {
        let (vendor_part, name_part) = id.0.split_once('/')?;
        if name_part.is_empty() {
            return None;
        }
        let vendor = if vendor_part == UNKNOWN {
            self.vendor_from_name(name_part)
        } else {
            self.vendor_by_key
                .get(&compact(&vendor_slug(vendor_part)))
                .copied()
        };
        let Some(v) = vendor else {
            if vendor_part != UNKNOWN {
                return None;
            }
            // No vendor anywhere: a unique catalog name still places it.
            return match self
                .product_by_name
                .get(&compact(name_part))
                .map(Vec::as_slice)
            {
                Some(&[only]) => Some(self.resolved_product(only)),
                _ => None,
            };
        };
        let (name, stripped) = self.strip_vendor(v, name_part);
        if let Some(&p) = self.product_by_key.get(&(v, compact(&name))) {
            return Some(self.resolved_product(p));
        }
        let vendor = &self.file.vendors[v];
        // Glued prefixes are only stripped for matching, not in the ID.
        let name = if stripped == Strip::Words {
            name
        } else {
            name_part.to_owned()
        };
        Some(Resolved {
            id: ProductId(format!("{}/{}", vendor.id, name)),
            vendor: vendor.name.clone(),
            product: None,
            stripped_name: stripped == Strip::Words,
        })
    }

    fn resolved_product(&self, i: usize) -> Resolved {
        let p = &self.file.products[i];
        Resolved {
            id: self.product_id(p),
            vendor: self
                .vendor(&p.vendor)
                .map_or_else(String::new, |v| v.name.clone()),
            product: Some(p.clone()),
            stripped_name: false,
        }
    }

    /// A vendor named at the start of an unknown-vendor product name, as whole words.
    fn vendor_from_name(&self, name_slug: &str) -> Option<usize> {
        self.vendor_slugs
            .iter()
            .enumerate()
            .flat_map(|(i, slugs)| slugs.iter().map(move |s| (i, s)))
            .filter(|(_, s)| {
                name_slug
                    .strip_prefix(s.as_str())
                    .is_some_and(|rest| rest.len() > 1 && rest.starts_with('-'))
            })
            .max_by_key(|(_, s)| s.len())
            .map(|(i, _)| i)
    }

    /// Take vendor `v`'s name off the front of a product name slug.
    fn strip_vendor(&self, v: usize, name_slug: &str) -> (String, Strip) {
        for s in &self.vendor_slugs[v] {
            if let Some(rest) = name_slug.strip_prefix(s.as_str())
                && let Some(rest) = rest.strip_prefix('-')
                && !rest.is_empty()
            {
                return (rest.to_owned(), Strip::Words);
            }
        }
        let glued = compact(name_slug);
        for s in &self.vendor_slugs[v] {
            let key = compact(s);
            if key.len() >= MIN_GLUED_PREFIX
                && let Some(rest) = glued.strip_prefix(&key)
                && !rest.is_empty()
            {
                return (rest.to_owned(), Strip::Glued);
            }
        }
        (name_slug.to_owned(), Strip::None)
    }

    /// Rewrite plug-in product IDs (and what templates require) to the
    /// catalog's, and use its vendor and product names. Returns how many
    /// distinct IDs changed. Products that end up with the same ID merge.
    pub fn canonicalize(&self, m: &mut Manifest) -> usize {
        let plugin_ids: Vec<ProductId> = m
            .items
            .iter()
            .filter(|i| i.kind == ItemKind::Plugin)
            .filter_map(|i| i.product_id.clone())
            .chain(m.items.iter().flat_map(|i| i.requires.iter().cloned()))
            .collect();
        let mut resolved: BTreeMap<ProductId, Resolved> = BTreeMap::new();
        for id in plugin_ids {
            if !resolved.contains_key(&id)
                && let Some(r) = self.resolve(&id)
            {
                resolved.insert(id, r);
            }
        }
        let changed = resolved.iter().filter(|(old, r)| **old != r.id).count();

        for item in &mut m.items {
            if item.kind == ItemKind::Plugin
                && let Some(r) = item.product_id.as_ref().and_then(|id| resolved.get(id))
            {
                item.product_id = Some(r.id.clone());
            }
            if item.requires.iter().any(|id| resolved.contains_key(id)) {
                for id in &mut item.requires {
                    if let Some(r) = resolved.get(id) {
                        *id = r.id.clone();
                    }
                }
                item.requires.sort();
                item.requires.dedup();
            }
        }

        let products = std::mem::take(&mut m.products);
        for mut p in products {
            if let Some(r) = resolved.get(&p.canonical_id) {
                p.canonical_id = r.id.clone();
                p.vendor = r.vendor.clone();
                if let Some(cp) = &r.product {
                    p.name = cp.name.clone();
                } else if r.stripped_name {
                    p.name = strip_display_prefix(&p.name, &self.vendor_names(&r.vendor));
                }
            }
            if m.product(&p.canonical_id).is_none() {
                m.products.push(p);
            }
        }
        changed
    }

    fn vendor_names(&self, display: &str) -> Vec<String> {
        self.file
            .vendors
            .iter()
            .filter(|v| v.name == display)
            .flat_map(|v| std::iter::once(v.name.clone()).chain(v.aliases.iter().cloned()))
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Strip {
    None,
    /// "FabFilter Pro-Q 4" -> "Pro-Q 4".
    Words,
    /// "ValhallaRoom" -> "Room", for matching only.
    Glued,
}

/// "FabFilter Pro-Q 4" -> "Pro-Q 4" when "FabFilter" is one of `vendors`.
fn strip_display_prefix(name: &str, vendors: &[String]) -> String {
    let mut best = name;
    for v in vendors {
        if name.len() > v.len()
            && name.is_char_boundary(v.len())
            && name[..v.len()].eq_ignore_ascii_case(v)
        {
            let rest = name[v.len()..].trim_start_matches([' ', '-', '_', ':']);
            if !rest.is_empty() && rest.len() < best.len() && name[v.len()..] != *rest {
                best = rest;
            }
        }
    }
    best.to_owned()
}

/// Lowercase alphanumeric words joined by `-`, the same as the scanner's slugs.
pub fn slug(s: &str) -> String {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join("-")
}

/// Vendor slug without company-form suffixes, matching the scanner's.
fn vendor_slug(v: &str) -> String {
    const SUFFIXES: &[&str] = &[
        "inc",
        "ltd",
        "llc",
        "gmbh",
        "ag",
        "ab",
        "co",
        "corp",
        "corporation",
        "limited",
        "sa",
        "bv",
        "srl",
        "kg",
        "pty",
    ];
    let mut words: Vec<String> = slug(v).split('-').map(str::to_owned).collect();
    while words.len() > 1 && words.last().is_some_and(|w| SUFFIXES.contains(&w.as_str())) {
        words.pop();
    }
    words.join("-")
}

fn compact(slug: &str) -> String {
    slug.chars().filter(|c| *c != '-').collect()
}

fn current_cell() -> &'static RwLock<Arc<Catalog>> {
    static CURRENT: OnceLock<RwLock<Arc<Catalog>>> = OnceLock::new();
    CURRENT.get_or_init(|| RwLock::new(Arc::new(Catalog::bundled())))
}

/// The catalog in use: the bundled one, or a newer one passed to [`install`].
pub fn current() -> Arc<Catalog> {
    current_cell()
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

/// Use `catalog` from now on if it is newer than the one in use. Returns
/// whether it was installed.
pub fn install(catalog: Catalog) -> bool {
    let mut cur = current_cell().write().unwrap_or_else(|e| e.into_inner());
    if catalog.updated() > cur.updated() {
        *cur = Arc::new(catalog);
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(s: &str) -> ProductId {
        ProductId(s.to_owned())
    }

    fn resolve(s: &str) -> Option<String> {
        Catalog::bundled().resolve(&id(s)).map(|r| r.id.0)
    }

    #[test]
    fn bundled_catalog_loads() {
        let c = Catalog::bundled();
        assert!(c.file().vendors.len() > 40);
        assert!(c.file().products.len() > 50);
    }

    #[test]
    fn spellings_of_one_product_share_an_id() {
        for raw in [
            "fabfilter/pro-q-4",
            "fabfilter/fabfilter-pro-q-4",
            "fabfilter-software-instruments/pro-q-4",
            "unknown/fabfilter-pro-q-4",
            "unknown/pro-q-4",
        ] {
            assert_eq!(resolve(raw).as_deref(), Some("fabfilter/pro-q-4"), "{raw}");
        }
        for raw in [
            "valhalla-dsp/valhallavintageverb",
            "valhalla-dsp/valhalla-vintage-verb",
            "valhalla/vintageverb",
        ] {
            assert_eq!(
                resolve(raw).as_deref(),
                Some("valhalla-dsp/valhallavintageverb"),
                "{raw}"
            );
        }
        assert_eq!(
            resolve("tokyo-dawn-labs/tdr-nova").as_deref(),
            Some("tokyo-dawn-records/tdr-nova")
        );
        assert_eq!(
            resolve("brainworx/bx-digital-v3").as_deref(),
            Some("plugin-alliance/bx-digital-v3")
        );
    }

    #[test]
    fn unknown_products_keep_their_name_under_the_vendor() {
        assert_eq!(
            resolve("fabfilter/fabfilter-future-thing").as_deref(),
            Some("fabfilter/future-thing")
        );
        // Glued prefixes help matching but never rewrite the name.
        assert_eq!(
            resolve("valhalla-dsp/valhallanewthing").as_deref(),
            Some("valhalla-dsp/valhallanewthing")
        );
        // Short aliases only strip whole words.
        assert_eq!(
            resolve("native-instruments/nice-drums").as_deref(),
            Some("native-instruments/nice-drums")
        );
    }

    #[test]
    fn unknown_vendors_are_left_alone() {
        assert_eq!(resolve("acme-audio/acme-verb"), None);
        assert_eq!(resolve("unknown/acme-verb"), None);
        assert_eq!(resolve("not-an-id"), None);
    }

    #[test]
    fn newer_catalogs_replace_older_ones_only() {
        let mut file = Catalog::bundled().file().clone();
        file.updated = "2000-01-01".into();
        assert!(!install(Catalog::new(file).unwrap()));
        let mut file = Catalog::bundled().file().clone();
        file.updated = "9999-01-01".into();
        assert!(install(Catalog::new(file).unwrap()));
        assert_eq!(current().updated(), "9999-01-01");
    }

    #[test]
    fn newer_format_is_refused() {
        let err = Catalog::from_json(r#"{"catalog_version": 99, "updated": "x"}"#).unwrap_err();
        assert!(matches!(err, CatalogError::UnsupportedVersion(99)));
    }

    #[test]
    fn display_prefix() {
        let v = vec!["FabFilter".to_owned()];
        assert_eq!(
            strip_display_prefix("FabFilter Future Thing", &v),
            "Future Thing"
        );
        assert_eq!(strip_display_prefix("FabFilterThing", &v), "FabFilterThing");
        assert_eq!(strip_display_prefix("FabFilter", &v), "FabFilter");
    }

    #[test]
    fn canonicalize_merges_spellings_and_fixes_requires() {
        use patchpilot_core::{
            Arch, Item, ItemKind, MachineInfo, Os, PathMap, PluginFormat, PortablePath, Product,
        };
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
        for (pid, vendor, name, format) in [
            (
                "fabfilter/fabfilter-pro-q-4",
                "FabFilter",
                "FabFilter Pro-Q 4",
                PluginFormat::Vst3,
            ),
            (
                "fabfilter-software-instruments/pro-q-4",
                "FabFilter Software Instruments",
                "Pro-Q 4",
                PluginFormat::Au,
            ),
            (
                "fabfilter/fabfilter-new-thing",
                "FabFilter",
                "FabFilter New Thing",
                PluginFormat::Vst3,
            ),
        ] {
            m.products.push(Product {
                canonical_id: pid.into(),
                vendor: vendor.into(),
                name: name.into(),
            });
            let mut item = Item::plugin(format, Os::Macos, PortablePath::logical("X", pid));
            item.product_id = Some(pid.into());
            m.items.push(item);
        }
        let mut template = Item::plugin(
            PluginFormat::Vst3,
            Os::Macos,
            PortablePath::logical("T", "t"),
        );
        template.kind = ItemKind::DawAsset;
        template.format = None;
        template.requires = vec![id("fabfilter/fabfilter-pro-q-4"), id("fabfilter/pro-q-4")];
        m.items.push(template);

        assert_eq!(Catalog::bundled().canonicalize(&mut m), 3);
        let names: Vec<(&str, &str, &str)> = m
            .products
            .iter()
            .map(|p| {
                (
                    p.canonical_id.0.as_str(),
                    p.vendor.as_str(),
                    p.name.as_str(),
                )
            })
            .collect();
        assert_eq!(
            names,
            [
                ("fabfilter/pro-q-4", "FabFilter", "Pro-Q 4"),
                ("fabfilter/new-thing", "FabFilter", "New Thing"),
            ]
        );
        assert_eq!(m.items[3].requires, [id("fabfilter/pro-q-4")]);
    }
}
