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
    let path = bundle.join("Contents").join("Info.plist");
    if !path.is_file() {
        return Ok(None);
    }
    let value = plist::Value::from_file(&path).map_err(|e| ReadError::Plist {
        path: path.clone(),
        message: e.to_string(),
    })?;
    let Some(dict) = value.as_dictionary() else {
        return Err(ReadError::Plist {
            path,
            message: "top level is not a dictionary".into(),
        });
    };
    Ok(Some(BundleInfo::from_dict(dict)))
}

impl BundleInfo {
    /// Read keys leniently: vendors sometimes write versions as numbers, and a
    /// bad value for one key shouldn't lose the others.
    pub(crate) fn from_dict(dict: &plist::Dictionary) -> Self {
        let get = |key: &str| -> Option<String> {
            let s = match dict.get(key)? {
                plist::Value::String(s) => s.clone(),
                plist::Value::Integer(i) => i.to_string(),
                plist::Value::Real(r) => r.to_string(),
                _ => return None,
            };
            let s = s.trim();
            (!s.is_empty()).then(|| s.to_owned())
        };
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
