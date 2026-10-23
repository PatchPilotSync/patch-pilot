use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{MANIFEST_SCHEMA_VERSION, PathMap, PortablePath};

/// Operating system of a machine or item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Os {
    Windows,
    Macos,
}

impl Os {
    /// The OS this build is running on, if it is one we support.
    pub fn current() -> Option<Self> {
        if cfg!(windows) {
            Some(Self::Windows)
        } else if cfg!(target_os = "macos") {
            Some(Self::Macos)
        } else {
            None
        }
    }

    pub fn separator(self) -> char {
        match self {
            Self::Windows => '\\',
            Self::Macos => '/',
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Arch {
    X86_64,
    Aarch64,
    /// macOS universal binary, or a Windows bundle carrying both.
    Universal,
}

/// What kind of thing an [`Item`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    Plugin,
    /// A DAW template, rack, preset, script, theme or project.
    DawAsset,
    VendorApp,
    /// iLok License Manager, PACE and similar. Detected, never authenticated.
    LicenseTool,
    Folder,
    Config,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PluginFormat {
    Vst2,
    Vst3,
    Au,
    Aax,
    Clap,
    Jsfx,
}

/// How sure a scanner is that an item belongs to its product.
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    /// Guessed from a file or folder name.
    Low,
    /// From a version resource or bundle plist.
    #[default]
    Medium,
    /// From a stable identifier: VST3 class ID, AU type/subtype/manufacturer, CLAP ID.
    High,
}

/// Canonical product identifier, shared by every format and OS of one product.
///
/// Stable once assigned; identity resolution (in `patchpilot-parity`) decides
/// which items share one.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(transparent)]
pub struct ProductId(pub String);

impl fmt::Display for ProductId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for ProductId {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}

/// A BLAKE3 hash of a user-owned file, written `blake3:<64 hex chars>`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ContentHash(String);

impl ContentHash {
    const PREFIX: &'static str = "blake3:";

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl JsonSchema for ContentHash {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "ContentHash".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "pattern": "^blake3:[0-9a-f]{64}$",
            "description": "A BLAKE3 hash of a user-owned file, written `blake3:<64 hex chars>`."
        })
    }
}

impl TryFrom<String> for ContentHash {
    type Error = ManifestError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        let hex = s
            .strip_prefix(Self::PREFIX)
            .ok_or_else(|| ManifestError::InvalidHash(s.clone()))?;
        if hex.len() == 64 && hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
            Ok(Self(s))
        } else {
            Err(ManifestError::InvalidHash(s))
        }
    }
}

impl From<ContentHash> for String {
    fn from(h: ContentHash) -> Self {
        h.0
    }
}

/// A product as the user thinks of it ("FabFilter Pro-Q 4"), across formats and OSes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Product {
    pub canonical_id: ProductId,
    pub vendor: String,
    pub name: String,
}

/// One thing installed on a machine: a plug-in binary in one format, a vendor
/// app, a template, a folder. Metadata only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Item {
    pub kind: ItemKind,
    /// `None` until identity resolution places the item.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub product_id: Option<ProductId>,
    /// Set for [`ItemKind::Plugin`] items.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<PluginFormat>,
    pub platform: Os,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arch: Option<Arch>,
    /// The version string exactly as the vendor wrote it. Compared in `patchpilot-parity`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub path: PortablePath,
    /// Only for user-owned files that may sync. Plug-ins and libraries are never hashed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<ContentHash>,
    /// Bytes on disk; for bundles and folders, the total.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// Last modification, Unix seconds UTC.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mtime: Option<i64>,
    /// True when the same file works on every OS (JSFX, Reaper themes, most presets).
    #[serde(default)]
    pub platform_portable: bool,
    #[serde(default)]
    pub confidence: Confidence,
    /// Products this item needs to work, sorted and unique: for a DAW
    /// template, rack or project, the plug-ins it uses (readiness, #37).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requires: Vec<ProductId>,
}

impl Item {
    /// A plug-in item with only the required fields set.
    pub fn plugin(format: PluginFormat, platform: Os, path: PortablePath) -> Self {
        Self {
            kind: ItemKind::Plugin,
            product_id: None,
            format: Some(format),
            platform,
            arch: None,
            version: None,
            path,
            content_hash: None,
            size: None,
            mtime: None,
            platform_portable: false,
            confidence: Confidence::default(),
            requires: Vec::new(),
        }
    }
}

/// The machine a manifest describes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MachineInfo {
    /// Stable random ID, generated once per install. Not derived from hardware.
    pub id: String,
    /// User-chosen name, e.g. "Studio PC" or "Travel MacBook".
    pub name: String,
    pub os: Os,
    pub arch: Arch,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os_version: Option<String>,
    /// How this machine's logical roots map to folders.
    pub path_map: PathMap,
}

/// Everything one machine has, at one point in time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Manifest {
    /// Always [`MANIFEST_SCHEMA_VERSION`] when written by this build.
    pub schema_version: u32,
    pub machine: MachineInfo,
    /// When the scan finished, Unix seconds UTC.
    pub generated_at: i64,
    #[serde(default)]
    pub products: Vec<Product>,
    #[serde(default)]
    pub items: Vec<Item>,
}

impl Manifest {
    pub fn new(machine: MachineInfo, generated_at: i64) -> Self {
        Self {
            schema_version: MANIFEST_SCHEMA_VERSION,
            machine,
            generated_at,
            products: Vec::new(),
            items: Vec::new(),
        }
    }

    /// Parse a manifest, refusing schema versions newer than this build understands.
    pub fn from_json(json: &str) -> Result<Self, ManifestError> {
        #[derive(Deserialize)]
        struct Header {
            schema_version: u32,
        }
        let Header { schema_version } = serde_json::from_str(json)?;
        if schema_version > MANIFEST_SCHEMA_VERSION {
            return Err(ManifestError::UnsupportedVersion(schema_version));
        }
        Ok(serde_json::from_str(json)?)
    }

    pub fn to_json_pretty(&self) -> String {
        serde_json::to_string_pretty(self).expect("manifest serializes")
    }

    pub fn product(&self, id: &ProductId) -> Option<&Product> {
        self.products.iter().find(|p| &p.canonical_id == id)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error(
        "manifest schema version {0} is newer than this build supports ({MANIFEST_SCHEMA_VERSION}); update Patch Pilot"
    )]
    UnsupportedVersion(u32),
    #[error("invalid content hash {0:?}: expected blake3:<64 lowercase hex chars>")]
    InvalidHash(String),
    #[error("invalid manifest JSON: {0}")]
    Json(#[from] serde_json::Error),
}
