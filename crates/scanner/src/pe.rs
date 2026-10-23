//! Windows PE version resources, read from the file bytes.
//!
//! The file is memory-mapped read-only as data (never as an executable image)
//! and only the header and resource directory are parsed. No code runs.

use std::path::Path;

use patchpilot_core::Arch;
use pelite::{FileMap, PeFile};

use crate::ReadError;

/// The `VS_VERSIONINFO` fields we use, plus the machine type.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PeVersion {
    pub product_name: Option<String>,
    pub company_name: Option<String>,
    pub file_description: Option<String>,
    /// `FileVersion` string as the vendor wrote it.
    pub file_version: Option<String>,
    /// `ProductVersion` string as the vendor wrote it.
    pub product_version: Option<String>,
    /// Numeric `VS_FIXEDFILEINFO` product version, `major.minor.patch.build`.
    pub fixed_product_version: Option<String>,
    pub arch: Option<Arch>,
}

impl PeVersion {
    /// The best version string: the vendor's product version, then file
    /// version, then the numeric fixed version.
    pub fn version(&self) -> Option<&str> {
        self.product_version
            .as_deref()
            .or(self.file_version.as_deref())
            .or(self.fixed_product_version.as_deref())
    }
}

pub fn read_pe_version(path: &Path) -> Result<PeVersion, ReadError> {
    let map = FileMap::open(path).map_err(|source| ReadError::Io {
        path: path.to_owned(),
        source,
    })?;
    parse_pe_version(map.as_ref(), path)
}

pub(crate) fn parse_pe_version(bytes: &[u8], path: &Path) -> Result<PeVersion, ReadError> {
    let pe_err = |e: pelite::Error| ReadError::Pe {
        path: path.to_owned(),
        message: e.to_string(),
    };
    let file = PeFile::from_bytes(bytes).map_err(pe_err)?;
    let arch = match file.file_header().Machine {
        0x8664 => Some(Arch::X86_64),
        0xAA64 => Some(Arch::Aarch64),
        _ => None,
    };
    let no_info = || ReadError::NoVersionInfo {
        path: path.to_owned(),
    };
    let info = file
        .resources()
        .map_err(|_| no_info())?
        .version_info()
        .map_err(|_| no_info())?;

    let mut out = PeVersion {
        arch,
        fixed_product_version: info.fixed().map(|f| {
            let v = f.dwProductVersion;
            format!("{}.{}.{}.{}", v.Major, v.Minor, v.Patch, v.Build)
        }),
        ..PeVersion::default()
    };
    // Prefer US English, then whatever the file declares first.
    let langs = info.translation();
    let lang = langs
        .iter()
        .find(|l| l.lang_id == 0x0409)
        .or(langs.first())
        .copied();
    if let Some(lang) = lang {
        let get = |key| {
            info.value(lang, key)
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
        };
        out.product_name = get("ProductName");
        out.company_name = get("CompanyName");
        out.file_description = get("FileDescription");
        out.file_version = get("FileVersion");
        out.product_version = get("ProductVersion");
    }
    Ok(out)
}
