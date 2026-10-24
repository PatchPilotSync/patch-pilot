//! Audio Unit identity from `.component` bundles (macOS), without loading.
//!
//! Modern AUs list their components under `AudioComponents` in `Info.plist`.
//! Each has a four-char type, subtype and manufacturer code: together they are
//! the AU's stable identity. One bundle may hold several components.

use std::path::Path;

use patchpilot_core::Confidence;

use crate::plist_info::{BundleInfo, plist_string, read_plist_dict};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuComponent {
    /// Four-char type code, e.g. `aufx` (effect), `aumu` (instrument), `aumf`.
    pub au_type: String,
    pub subtype: String,
    pub manufacturer: String,
    /// The full `name`, conventionally `Manufacturer: Product`.
    pub name: String,
    /// The part of `name` before `: `, if any.
    pub vendor: Option<String>,
    /// The part of `name` after `: `, or all of it.
    pub product_name: String,
    /// Decoded from the packed `0xMMMMmmbb` integer, e.g. `2.1.0`.
    pub version: Option<String>,
    pub description: Option<String>,
}

impl AuComponent {
    /// `type:subtype:manufacturer`, e.g. `aufx:Verb:Acme`. Stable across versions.
    pub fn component_id(&self) -> String {
        format!("{}:{}:{}", self.au_type, self.subtype, self.manufacturer)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuInfo {
    /// Bundle name, or the folder name if the plist has none.
    pub name: String,
    pub bundle: BundleInfo,
    pub components: Vec<AuComponent>,
    pub confidence: Confidence,
    pub warnings: Vec<String>,
}

impl AuInfo {
    /// The bundle's marketing version, then the first component's, then the
    /// bundle's build version. Some vendors (Arturia) leave out the marketing
    /// version and put the packed AU number in `CFBundleVersion`, which would
    /// show as "65797" instead of the component's decoded "1.1.5".
    pub fn version(&self) -> Option<&str> {
        self.bundle
            .short_version
            .as_deref()
            .or_else(|| self.components.iter().find_map(|c| c.version.as_deref()))
            .or(self.bundle.bundle_version.as_deref())
    }
}

/// Read a `.component` bundle. Never fails: missing metadata lowers confidence.
pub fn read_au(path: &Path) -> AuInfo {
    let mut info = AuInfo {
        name: path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default(),
        bundle: BundleInfo::default(),
        components: Vec::new(),
        confidence: Confidence::Low,
        warnings: Vec::new(),
    };
    let dict = match read_plist_dict(path) {
        Ok(Some(dict)) => dict,
        Ok(None) => {
            info.warnings
                .push(format!("{}: no Contents/Info.plist", path.display()));
            return info;
        }
        Err(e) => {
            info.warnings.push(e.to_string());
            return info;
        }
    };
    info.bundle = BundleInfo::from_dict(&dict);
    if let Some(name) = info.bundle.best_name() {
        info.name = name.to_owned();
    }
    info.confidence = Confidence::Medium;

    let entries = dict
        .get("AudioComponents")
        .and_then(plist::Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    for (i, entry) in entries.iter().enumerate() {
        match entry.as_dictionary().map(parse_component) {
            Some(Ok(c)) => info.components.push(c),
            Some(Err(why)) => info
                .warnings
                .push(format!("{}: AudioComponents[{i}]: {why}", path.display())),
            None => info.warnings.push(format!(
                "{}: AudioComponents[{i}] is not a dictionary",
                path.display()
            )),
        }
    }
    if !info.components.is_empty() {
        info.confidence = Confidence::High;
    }
    info
}

fn parse_component(d: &plist::Dictionary) -> Result<AuComponent, String> {
    let code = |key: &str| -> Result<String, String> {
        let s = match d.get(key) {
            Some(plist::Value::String(s)) => s.clone(),
            _ => return Err(format!("missing {key}")),
        };
        // Four-char codes may contain spaces, so don't trim.
        if s.chars().count() == 4 {
            Ok(s)
        } else {
            Err(format!("{key} {s:?} is not a four-char code"))
        }
    };
    let au_type = code("type")?;
    let subtype = code("subtype")?;
    let manufacturer = code("manufacturer")?;
    let name = plist_string(d.get("name")).unwrap_or_default();
    let (vendor, product_name) = match name.split_once(':') {
        Some((v, p)) if !v.trim().is_empty() && !p.trim().is_empty() => {
            (Some(v.trim().to_owned()), p.trim().to_owned())
        }
        _ => (None, name.clone()),
    };
    Ok(AuComponent {
        au_type,
        subtype,
        manufacturer,
        name,
        vendor,
        product_name,
        version: d.get("version").and_then(decode_version),
        description: plist_string(d.get("description")),
    })
}

/// AU versions are packed `0xMMMMmmbb`. Accept integers or numeric strings
/// (decimal or `0x` hex); anything else is kept as written.
fn decode_version(v: &plist::Value) -> Option<String> {
    let n = match v {
        plist::Value::Integer(i) => i.as_unsigned()?,
        plist::Value::String(s) => {
            let s = s.trim();
            let parsed = match s.strip_prefix("0x").or(s.strip_prefix("0X")) {
                Some(hex) => u64::from_str_radix(hex, 16).ok(),
                None => s.parse().ok(),
            };
            match parsed {
                Some(n) => n,
                None => return (!s.is_empty()).then(|| s.to_owned()),
            }
        }
        _ => return None,
    };
    let n = u32::try_from(n).ok()?;
    Some(format!("{}.{}.{}", n >> 16, (n >> 8) & 0xff, n & 0xff))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_packed_versions() {
        let int = |i: u64| plist::Value::Integer(i.into());
        assert_eq!(decode_version(&int(0x0002_0100)).as_deref(), Some("2.1.0"));
        assert_eq!(decode_version(&int(65536)).as_deref(), Some("1.0.0"));
        let s = |s: &str| plist::Value::String(s.into());
        assert_eq!(decode_version(&s("0x00010203")).as_deref(), Some("1.2.3"));
        assert_eq!(decode_version(&s("131328")).as_deref(), Some("2.1.0"));
        assert_eq!(decode_version(&s("2.1b3")).as_deref(), Some("2.1b3"));
    }

    #[test]
    fn component_version_beats_packed_bundle_version() {
        let component = |version: Option<&str>| AuComponent {
            au_type: "aumu".into(),
            subtype: "Acid".into(),
            manufacturer: "Artu".into(),
            name: "Arturia: Acid V".into(),
            vendor: Some("Arturia".into()),
            product_name: "Acid V".into(),
            version: version.map(str::to_owned),
            description: None,
        };
        let mut info = AuInfo {
            name: "Acid V".into(),
            bundle: BundleInfo {
                bundle_version: Some("65797".into()),
                ..Default::default()
            },
            components: vec![component(Some("1.1.5"))],
            confidence: Confidence::High,
            warnings: vec![],
        };
        assert_eq!(info.version(), Some("1.1.5"));
        info.bundle.short_version = Some("1.1.5.2201".into());
        assert_eq!(info.version(), Some("1.1.5.2201"));
        info.bundle.short_version = None;
        info.components = vec![component(None)];
        assert_eq!(info.version(), Some("65797"));
    }
}
