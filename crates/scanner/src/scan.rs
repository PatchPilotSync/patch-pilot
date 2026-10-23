//! Turn a machine's plug-in folders and vendor apps into a [`Manifest`].
//!
//! Product identity v1 is name-based so formats and OSes line up: the
//! canonical ID is `<vendor-slug>/<name-slug>` (e.g. `acme-audio/acme-verb`),
//! or `unknown/<name-slug>` when no vendor could be read. Parity aligns
//! `unknown/` IDs with a vendor's ID of the same name when it's unambiguous.

use std::path::Path;

use patchpilot_core::{
    Arch, Confidence, Item, MachineInfo, Manifest, Os, PluginFormat, Product, ProductId,
};

use crate::vendor_apps::{Detection, VendorAppStatus};
use crate::{
    Candidate, PluginRoot, WalkWarning, enumerate, normalize_name, path_map, read_aax, read_au,
    read_vst2, read_vst3,
};

/// What [`scan_machine`] needs to know about the machine.
#[derive(Debug, Clone)]
pub struct ScanInput {
    pub id: String,
    pub name: String,
    pub os: Os,
    pub arch: Arch,
    pub os_version: Option<String>,
    pub roots: Vec<PluginRoot>,
    /// Vendor app results from [`crate::detect_windows`] / [`crate::detect_macos`].
    pub vendor_apps: Vec<VendorAppStatus>,
    /// Unix seconds.
    pub now: i64,
}

#[derive(Debug)]
pub struct ScanOutput {
    pub manifest: Manifest,
    /// Walk problems and per-plug-in metadata warnings, for a log.
    pub warnings: Vec<String>,
}

pub fn scan_machine(input: ScanInput) -> ScanOutput {
    let map = path_map(input.os, &input.roots);
    let found = enumerate(&input.roots);
    let mut warnings: Vec<String> = found.warnings.iter().map(walk_warning).collect();

    let mut manifest = Manifest::new(
        MachineInfo {
            id: input.id,
            name: input.name,
            os: input.os,
            arch: input.arch,
            os_version: input.os_version,
            path_map: map.clone(),
        },
        input.now,
    );

    for c in &found.candidates {
        let read = read_candidate(c);
        warnings.extend(read.warnings);
        let id = product_id(read.vendor.as_deref(), &read.name);
        if manifest.product(&id).is_none() {
            manifest.products.push(Product {
                canonical_id: id.clone(),
                vendor: read.vendor.clone().unwrap_or_default(),
                name: read.name.clone(),
            });
        }
        let mut item = Item::plugin(
            c.format,
            input.os,
            map.to_portable(&c.path.to_string_lossy()),
        );
        item.product_id = Some(id);
        item.version = read.version;
        item.arch = read.arch;
        item.confidence = read.confidence;
        item.mtime = mtime(&c.path);
        manifest.items.push(item);
    }

    for status in &input.vendor_apps {
        let Detection::Detected { version, location } = &status.detection else {
            continue;
        };
        let a = status.adapter;
        let id = ProductId(format!("app/{}", a.id));
        manifest.products.push(Product {
            canonical_id: id.clone(),
            vendor: String::new(),
            name: a.name.to_owned(),
        });
        let path = location.clone().unwrap_or_else(|| a.name.to_owned());
        manifest.items.push(Item {
            kind: a.kind,
            product_id: Some(id),
            format: None,
            platform: input.os,
            arch: None,
            version: version.clone(),
            path: map.to_portable(&path),
            content_hash: None,
            size: None,
            mtime: None,
            platform_portable: false,
            confidence: Confidence::Medium,
        });
    }

    ScanOutput { manifest, warnings }
}

struct Read {
    name: String,
    vendor: Option<String>,
    version: Option<String>,
    arch: Option<Arch>,
    confidence: Confidence,
    warnings: Vec<String>,
}

fn read_candidate(c: &Candidate) -> Read {
    match c.format {
        PluginFormat::Vst3 => {
            let i = read_vst3(&c.path);
            Read {
                name: normalize_name(&i.name).name,
                vendor: i.vendor,
                version: i.version,
                arch: i.arch,
                confidence: i.confidence,
                warnings: i.warnings,
            }
        }
        PluginFormat::Au => {
            let i = read_au(&c.path);
            let first = i.components.first();
            let version = i.version().map(str::to_owned);
            Read {
                name: first.map_or_else(
                    || normalize_name(&i.name).name,
                    |c| normalize_name(&c.product_name).name,
                ),
                vendor: first.and_then(|c| c.vendor.clone()),
                version,
                arch: None,
                confidence: i.confidence,
                warnings: i.warnings,
            }
        }
        PluginFormat::Vst2 | PluginFormat::Aax => {
            let m = if c.format == PluginFormat::Vst2 {
                read_vst2(&c.path)
            } else {
                read_aax(&c.path)
            };
            Read {
                name: m.name,
                vendor: m.vendor,
                version: m.version,
                arch: m.arch,
                confidence: m.confidence,
                warnings: m.warnings,
            }
        }
        // CLAP has no static metadata (see #8); name only until that's decided.
        PluginFormat::Clap | PluginFormat::Jsfx => {
            let stem = c
                .path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let n = normalize_name(&stem);
            Read {
                name: n.name,
                vendor: None,
                version: None,
                arch: n.arch,
                confidence: Confidence::Low,
                warnings: Vec::new(),
            }
        }
    }
}

/// `<vendor-slug>/<name-slug>`, or `unknown/<name-slug>`.
pub fn product_id(vendor: Option<&str>, name: &str) -> ProductId {
    let vendor = vendor.map(vendor_slug).filter(|v| !v.is_empty());
    ProductId(format!(
        "{}/{}",
        vendor.as_deref().unwrap_or("unknown"),
        slug(name)
    ))
}

/// Lowercase ASCII alphanumeric words joined by `-`.
pub fn slug(s: &str) -> String {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join("-")
}

/// Vendor slug without company-form suffixes, so "Acme Audio GmbH" and
/// "Acme Audio, Inc." agree.
fn vendor_slug(v: &str) -> String {
    const SUFFIXES: &[&str] = &[
        "inc",
        "ltd",
        "llc",
        "gmbh",
        "ag",
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

fn mtime(path: &Path) -> Option<i64> {
    let t = std::fs::metadata(path).ok()?.modified().ok()?;
    let secs = t.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    i64::try_from(secs).ok()
}

fn walk_warning(w: &WalkWarning) -> String {
    match &w.path {
        Some(p) => format!("{}: {}", p.display(), w.message),
        None => w.message.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_ids() {
        assert_eq!(
            product_id(Some("Acme Audio GmbH"), "Acme Verb").0,
            "acme-audio/acme-verb"
        );
        assert_eq!(
            product_id(Some("Acme Audio, Inc."), "Acme Verb").0,
            "acme-audio/acme-verb"
        );
        assert_eq!(product_id(None, "Mystery Synth").0, "unknown/mystery-synth");
        assert_eq!(product_id(Some("  "), "X").0, "unknown/x");
        assert_eq!(slug("Pro-Q 4 (Stereo)"), "pro-q-4-stereo");
    }
}
