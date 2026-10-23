//! VST2, AAX and CLAP file metadata without loading.
//!
//! - Windows VST2: a `.dll`; read its PE version resource.
//! - macOS VST2: a `.vst` bundle; read `Info.plist`.
//! - CLAP: a PE `.clap` file on Windows, a `.clap` bundle on macOS, read the
//!   same way. The real CLAP IDs come from [`crate::ClapCache`] when Reaper
//!   has scanned the file.
//! - AAX: a `.aaxplugin` bundle on both OSes. macOS has `Info.plist`; Windows
//!   keeps the PE at `Contents/x64/<name>.aaxplugin` (or `Contents/Win32/`).
//!
//! Names are cleaned with [`normalize_name`] so `Acme Comp x64.dll` and
//! `Acme Comp.vst` both read as "Acme Comp".

use std::path::{Path, PathBuf};

use patchpilot_core::{Arch, Confidence, PluginFormat};

use crate::names::{Channels, normalize_name};
use crate::pe::read_pe_version;
use crate::plist_info::read_bundle_info;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetaSource {
    Plist,
    PeResource,
    FileName,
}

/// What a VST2, AAX or CLAP plug-in file says about itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginMeta {
    pub format: PluginFormat,
    /// Normalized product name.
    pub name: String,
    /// The name before normalization, for display and debugging.
    pub raw_name: String,
    pub vendor: Option<String>,
    pub version: Option<String>,
    pub bundle_id: Option<String>,
    pub arch: Option<Arch>,
    pub channels: Option<Channels>,
    pub source: MetaSource,
    pub confidence: Confidence,
    pub warnings: Vec<String>,
}

pub fn read_vst2(path: &Path) -> PluginMeta {
    read_meta(path, PluginFormat::Vst2, None)
}

pub fn read_clap(path: &Path) -> PluginMeta {
    read_meta(path, PluginFormat::Clap, None)
}

pub fn read_aax(path: &Path) -> PluginMeta {
    let bundle_binary = |bundle: &Path| -> Option<PathBuf> {
        let file_name = bundle.file_name()?;
        ["x64", "Win32"]
            .iter()
            .map(|arch| bundle.join("Contents").join(arch).join(file_name))
            .find(|p| p.is_file())
    };
    read_meta(path, PluginFormat::Aax, Some(&bundle_binary))
}

type BinaryLocator<'a> = &'a dyn Fn(&Path) -> Option<PathBuf>;

fn read_meta(path: &Path, format: PluginFormat, locate_pe: Option<BinaryLocator>) -> PluginMeta {
    let mut meta = PluginMeta {
        format,
        name: String::new(),
        raw_name: path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default(),
        vendor: None,
        version: None,
        bundle_id: None,
        arch: None,
        channels: None,
        source: MetaSource::FileName,
        confidence: Confidence::Low,
        warnings: Vec::new(),
    };

    let pe_path = if path.is_dir() {
        locate_pe.and_then(|f| f(path))
    } else {
        Some(path.to_owned())
    };
    if let Some(pe_path) = pe_path {
        match read_pe_version(&pe_path) {
            Ok(pe) => {
                // FileDescription is often the friendlier name in VST2 DLLs.
                if let Some(n) = pe.product_name.as_ref().or(pe.file_description.as_ref()) {
                    meta.raw_name = n.clone();
                }
                meta.vendor = pe.company_name.clone();
                meta.version = pe.version().map(str::to_owned);
                meta.arch = pe.arch;
                meta.source = MetaSource::PeResource;
                meta.confidence = Confidence::Medium;
            }
            Err(e) => meta.warnings.push(e.to_string()),
        }
    }
    if path.is_dir() {
        match read_bundle_info(path) {
            Ok(Some(plist)) => {
                if let Some(n) = plist.best_name() {
                    meta.raw_name = n.to_owned();
                }
                meta.version = plist.version().map(str::to_owned).or(meta.version.take());
                meta.bundle_id = plist.identifier.clone();
                meta.source = MetaSource::Plist;
                meta.confidence = Confidence::Medium;
            }
            Ok(None) => {}
            Err(e) => meta.warnings.push(e.to_string()),
        }
    }

    let normalized = normalize_name(&meta.raw_name);
    // Tags in the file name still count when the metadata name is clean,
    // e.g. ProductName "Acme EQ" in "Acme EQ Mono.dll".
    let from_file = path
        .file_stem()
        .map(|s| normalize_name(&s.to_string_lossy()));
    meta.name = normalized.name;
    meta.channels = normalized
        .channels
        .or(from_file.as_ref().and_then(|n| n.channels));
    meta.arch = meta
        .arch
        .or(normalized.arch)
        .or(from_file.and_then(|n| n.arch));
    meta
}
