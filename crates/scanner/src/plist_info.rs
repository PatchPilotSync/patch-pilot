//! macOS bundle `Info.plist` (XML or binary).

use std::path::Path;

use crate::ReadError;

/// The standard bundle keys we use.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BundleInfo {
    pub name: Option<String>,
    pub display_name: Option<String>,
    pub identifier: Option<String>,
    pub short_version: Option<String>,
    pub bundle_version: Option<String>,
    pub executable: Option<String>,
    pub copyright: Option<String>,
}

impl BundleInfo {
    pub fn best_name(&self) -> Option<&str> {
        self.display_name.as_deref().or(self.name.as_deref())
    }

    /// Marketing version, then build version.
    pub fn version(&self) -> Option<&str> {
        self.short_version
            .as_deref()
            .or(self.bundle_version.as_deref())
    }
}

/// Read `Contents/Info.plist` of a bundle, if it has one.
pub fn read_bundle_info(bundle: &Path) -> Result<Option<BundleInfo>, ReadError> {
    Ok(read_plist_dict(bundle)?.map(|dict| BundleInfo::from_dict(&dict)))
}

/// The top-level dictionary of a bundle's `Contents/Info.plist`, if it has one.
pub(crate) fn read_plist_dict(bundle: &Path) -> Result<Option<plist::Dictionary>, ReadError> {
    let path = bundle.join("Contents").join("Info.plist");
    if !path.is_file() {
        return Ok(None);
    }
    let value = plist::Value::from_file(&path).map_err(|e| ReadError::Plist {
        path: path.clone(),
        message: e.to_string(),
    })?;
    match value.into_dictionary() {
        Some(dict) => Ok(Some(dict)),
        None => Err(ReadError::Plist {
            path,
            message: "top level is not a dictionary".into(),
        }),
    }
}

/// A plist value as a trimmed, non-empty string. Numbers are accepted because
/// vendors sometimes write versions that way.
pub(crate) fn plist_string(value: Option<&plist::Value>) -> Option<String> {
    let s = match value? {
        plist::Value::String(s) => s.clone(),
        plist::Value::Integer(i) => i.to_string(),
        plist::Value::Real(r) => r.to_string(),
        _ => return None,
    };
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_owned())
}

impl BundleInfo {
    /// Read keys leniently: a bad value for one key shouldn't lose the others.
    pub(crate) fn from_dict(dict: &plist::Dictionary) -> Self {
        let get = |key: &str| plist_string(dict.get(key));
        Self {
            name: get("CFBundleName"),
            display_name: get("CFBundleDisplayName"),
            identifier: get("CFBundleIdentifier"),
            short_version: get("CFBundleShortVersionString"),
            bundle_version: get("CFBundleVersion"),
            executable: get("CFBundleExecutable"),
            copyright: get("NSHumanReadableCopyright"),
        }
    }
}
