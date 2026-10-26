use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{Manifest, ManifestError, Matcher, TargetProfile};

/// Version of the exchange bundle format this build reads and writes.
pub const BUNDLE_VERSION: u32 = 1;

/// A file that moves manifests between machines without a cloud (#38):
/// machines' latest scans, Primary Rig versions and target profiles.
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
    /// Suggestions for the Primary Rig the user chose not to add, so every
    /// linked machine stops offering them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ignored: Vec<IgnoredSuggestion>,
    /// What the user chose not to bring over or install on a machine, and
    /// what they took back, so every linked machine counts the same.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ignores: Vec<IgnoreEntry>,
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

/// A product version the user doesn't want in the Primary Rig ("Ignore" on a
/// suggestion). A newer version is suggested again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct IgnoredSuggestion {
    pub product_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Unix seconds UTC: when it was ignored, or un-ignored if `undone`.
    pub ignored_at: i64,
    /// Un-ignored: offered again. Kept so the change reaches linked machines.
    #[serde(default, skip_serializing_if = "is_false")]
    pub undone: bool,
}

/// Something the user chose to stop being asked about on a machine ("Ignore"
/// in Bring over, Get Up To Speed or Compare): a file, a folder, a step's
/// products. It leaves the machine's target like a profile's exclusion, so it
/// drops out of the parity score, the plan and every count.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct IgnoreEntry {
    /// The machine it applies to; `None` for every linked machine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub machine_id: Option<String>,
    /// What the user saw, e.g. "ReaTeam Scripts" or "Serum".
    pub label: String,
    /// Matches what it hides; anything one of them matches is left out.
    pub rules: Vec<Matcher>,
    /// Unix seconds UTC: when it was ignored, or un-ignored if `undone`.
    pub at: i64,
    /// Un-ignored. Kept so the change reaches linked machines.
    #[serde(default, skip_serializing_if = "is_false")]
    pub undone: bool,
}

impl IgnoreEntry {
    /// Identifies the entry for its machine: the same rules are one entry.
    pub fn key(&self) -> String {
        ignore_key(&self.rules)
    }
}

/// [`IgnoreEntry::key`] for these rules.
pub fn ignore_key(rules: &[Matcher]) -> String {
    serde_json::to_string(rules).expect("matchers serialize")
}

fn is_false(b: &bool) -> bool {
    !b
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
            ignored: Vec::new(),
            ignores: Vec::new(),
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
