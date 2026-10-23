use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{Manifest, ManifestError, TargetProfile};

/// Version of the exchange bundle format this build reads and writes.
pub const BUNDLE_VERSION: u32 = 1;

/// A file that moves manifests between machines without a cloud (#38):
/// machines' latest scans, Golden Profile versions and target profiles.
///
/// Metadata only, like a manifest: no plug-ins, installers or user files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Bundle {
    /// Always [`BUNDLE_VERSION`] when written by this build.
    pub bundle_version: u32,
    /// Unix seconds UTC.
    pub exported_at: i64,
    /// The machine that wrote the bundle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exported_from: Option<String>,
    #[serde(default)]
    pub machines: Vec<BundleMachine>,
    /// Oldest first.
    #[serde(default)]
    pub golden: Vec<BundleGolden>,
    /// User target profiles (built-in presets are not exported).
    #[serde(default)]
    pub profiles: Vec<TargetProfile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct BundleMachine {
    /// The machine's latest scan.
    pub manifest: Manifest,
    /// The target profile assigned to it, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct BundleGolden {
    /// Version number on the exporting machine.
    pub version: u32,
    pub created_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_machine_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub manifest: Manifest,
}

impl Bundle {
    pub fn new(exported_at: i64, exported_from: Option<String>) -> Self {
        Self {
            bundle_version: BUNDLE_VERSION,
            exported_at,
            exported_from,
            machines: Vec::new(),
            golden: Vec::new(),
            profiles: Vec::new(),
        }
    }

    /// Parse a bundle, refusing bundle or manifest versions newer than this
    /// build understands.
    pub fn from_json(json: &str) -> Result<Self, ManifestError> {
        #[derive(Deserialize)]
        struct Header {
            bundle_version: u32,
        }
        let Header { bundle_version } = serde_json::from_str(json)?;
        if bundle_version > BUNDLE_VERSION {
            return Err(ManifestError::UnsupportedBundleVersion(bundle_version));
        }
        let bundle: Self = serde_json::from_str(json)?;
        let manifests = bundle
            .machines
            .iter()
            .map(|m| &m.manifest)
            .chain(bundle.golden.iter().map(|g| &g.manifest));
        for m in manifests {
            if m.schema_version > crate::MANIFEST_SCHEMA_VERSION {
                return Err(ManifestError::UnsupportedVersion(m.schema_version));
            }
        }
        Ok(bundle)
    }

    pub fn to_json_pretty(&self) -> String {
        serde_json::to_string_pretty(self).expect("bundle serializes")
    }
}
