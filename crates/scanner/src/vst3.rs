//! VST3 identity without loading the plug-in.
//!
//! Sources, best first:
//! 1. `Contents/Resources/moduleinfo.json` (VST3 SDK 3.7.5+): class IDs, vendor, version.
//! 2. `Contents/Info.plist` (macOS bundles).
//! 3. The PE version resource of the binary (Windows bundles and legacy single-file `.vst3`).
//! 4. The bundle or file name, with low confidence.
//!
//! VST3 class IDs are the same on Windows and macOS, so they are the
//! cross-platform identity key.

use std::path::{Path, PathBuf};

use patchpilot_core::{Arch, Confidence};
use serde::Deserialize;

use crate::ReadError;
use crate::pe::read_pe_version;
use crate::plist_info::read_bundle_info;

/// Where the identity fields came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vst3Source {
    ModuleInfo,
    Plist,
    PeResource,
    FileName,
}

/// One class exported by the module's factory, from `moduleinfo.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vst3Class {
    /// 32 uppercase hex chars.
    pub cid: String,
    pub name: String,
    /// e.g. `Audio Module Class`, `Component Controller Editor Class`.
    pub category: String,
    pub sub_categories: Vec<String>,
    pub vendor: Option<String>,
    pub version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vst3Info {
    pub name: String,
    pub vendor: Option<String>,
    pub version: Option<String>,
    pub classes: Vec<Vst3Class>,
    /// macOS bundle identifier, when there is a plist.
    pub bundle_id: Option<String>,
    pub arch: Option<Arch>,
    pub source: Vst3Source,
    pub confidence: Confidence,
    /// Sources that existed but couldn't be read. Not fatal.
    pub warnings: Vec<String>,
}

impl Vst3Info {
    /// Class ID of the first audio processor class: the product's identity key.
    pub fn primary_cid(&self) -> Option<&str> {
        self.classes
            .iter()
            .find(|c| c.category == "Audio Module Class")
            .map(|c| c.cid.as_str())
    }
}

/// Read what can be read about a `.vst3` bundle folder or legacy single file.
///
/// Never fails: missing or unreadable metadata degrades to a lower-confidence
/// result, with the reasons in `warnings`.
pub fn read_vst3(path: &Path) -> Vst3Info {
    let mut info = Vst3Info {
        name: file_stem_name(path),
        vendor: None,
        version: None,
        classes: Vec::new(),
        bundle_id: None,
        arch: None,
        source: Vst3Source::FileName,
        confidence: Confidence::Low,
        warnings: Vec::new(),
    };
    // Fill lower-priority sources first so better ones overwrite them.
    if path.is_dir() {
        if let Some(binary) = windows_binary(path) {
            apply_pe(&mut info, &binary);
        }
        match read_bundle_info(path) {
            Ok(Some(plist)) => {
                if let Some(name) = plist.best_name() {
                    info.name = name.to_owned();
                }
                info.version = plist.version().map(str::to_owned).or(info.version);
                info.bundle_id = plist.identifier.clone();
                info.source = Vst3Source::Plist;
                info.confidence = Confidence::Medium;
            }
            Ok(None) => {}
            Err(e) => info.warnings.push(e.to_string()),
        }
        let module_info = path
            .join("Contents")
            .join("Resources")
            .join("moduleinfo.json");
        if module_info.is_file() {
            match read_module_info(&module_info) {
                Ok(m) => apply_module_info(&mut info, m),
                Err(e) => info.warnings.push(e.to_string()),
            }
        }
    } else {
        apply_pe(&mut info, path);
    }
    info
}

fn apply_pe(info: &mut Vst3Info, binary: &Path) {
    match read_pe_version(binary) {
        Ok(pe) => {
            if let Some(name) = &pe.product_name {
                info.name = name.clone();
            }
            info.vendor = pe.company_name.clone().or(info.vendor.take());
            info.version = pe.version().map(str::to_owned).or(info.version.take());
            info.arch = pe.arch;
            info.source = Vst3Source::PeResource;
            info.confidence = Confidence::Medium;
        }
        Err(e) => info.warnings.push(e.to_string()),
    }
}

fn apply_module_info(info: &mut Vst3Info, m: ModuleInfo) {
    if !m.name.trim().is_empty() {
        info.name = m.name.trim().to_owned();
    }
    let vendor = m.factory_info.vendor.filter(|v| !v.trim().is_empty());
    info.vendor = vendor.or(info.vendor.take());
    info.version = m
        .version
        .filter(|v| !v.trim().is_empty())
        .or(info.version.take());
    info.classes = m
        .classes
        .into_iter()
        .filter_map(|c| {
            let cid = normalize_cid(&c.cid)?;
            Some(Vst3Class {
                cid,
                name: c.name,
                category: c.category,
                sub_categories: c.sub_categories,
                vendor: c.vendor.filter(|v| !v.is_empty()),
                version: c.version.filter(|v| !v.is_empty()),
            })
        })
        .collect();
    info.source = Vst3Source::ModuleInfo;
    // Without a class ID, moduleinfo is no better than a plist.
    info.confidence = if info.primary_cid().is_some() {
        Confidence::High
    } else {
        Confidence::Medium
    };
}

/// The PE inside a Windows bundle: `Contents/<arch>-win/<name>.vst3`.
/// Prefers x64, then arm64 variants, then 32-bit.
fn windows_binary(bundle: &Path) -> Option<PathBuf> {
    let contents = bundle.join("Contents");
    let file_name = bundle.file_name()?;
    [
        "x86_64-win",
        "arm64x-win",
        "arm64ec-win",
        "arm64-win",
        "x86-win",
    ]
    .iter()
    .map(|arch| contents.join(arch).join(file_name))
    .find(|p| p.is_file())
}

fn file_stem_name(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Accept `ABCD...` or `{ABCD-...}` forms; return 32 uppercase hex chars.
fn normalize_cid(raw: &str) -> Option<String> {
    let hex: String = raw
        .chars()
        .filter(|c| !matches!(c, '{' | '}' | '-' | ' '))
        .collect();
    (hex.len() == 32 && hex.chars().all(|c| c.is_ascii_hexdigit()))
        .then(|| hex.to_ascii_uppercase())
}

#[derive(Debug, Deserialize)]
struct ModuleInfo {
    #[serde(rename = "Name", default)]
    name: String,
    #[serde(rename = "Version")]
    version: Option<String>,
    #[serde(rename = "Factory Info", default)]
    factory_info: FactoryInfo,
    #[serde(rename = "Classes", default)]
    classes: Vec<ModuleClass>,
}

#[derive(Debug, Default, Deserialize)]
struct FactoryInfo {
    #[serde(rename = "Vendor")]
    vendor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ModuleClass {
    #[serde(rename = "CID")]
    cid: String,
    #[serde(rename = "Name", default)]
    name: String,
    #[serde(rename = "Category", default)]
    category: String,
    #[serde(rename = "Sub Categories", default)]
    sub_categories: Vec<String>,
    #[serde(rename = "Vendor")]
    vendor: Option<String>,
    #[serde(rename = "Version")]
    version: Option<String>,
}

/// The SDK writes JSON5-tolerant files (comments, trailing commas), so parse as JSON5.
fn read_module_info(path: &Path) -> Result<ModuleInfo, ReadError> {
    let text = std::fs::read_to_string(path).map_err(|source| ReadError::Io {
        path: path.to_owned(),
        source,
    })?;
    let text = text.trim_start_matches('\u{feff}');
    json5::from_str(text).map_err(|e| ReadError::ModuleInfo {
        path: path.to_owned(),
        message: e.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_class_ids() {
        assert_eq!(
            normalize_cid("{5653544a-6365-4d61-6372-6f7665726200}").as_deref(),
            Some("5653544A63654D6163726F7665726200")
        );
        assert_eq!(normalize_cid("not-a-cid"), None);
        assert_eq!(normalize_cid("5653544A63654D6163726F76657262"), None);
    }
}
