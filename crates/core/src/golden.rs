use crate::Manifest;

/// One version of the Golden Profile (decision 2): the ideal rig as a
/// manifest that outlives any one machine.
///
/// Versions are never overwritten. Each one records the version it came
/// from, so the history is a chain `v1 ← v2 ← v3`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoldenVersion {
    pub info: GoldenVersionInfo,
    /// The ideal rig. Its `machine` names this version (`golden@<n>`,
    /// "Golden Rig v<n>"); items keep the platform they were scanned on.
    pub manifest: Manifest,
}

/// A Golden Profile version without its manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoldenVersionInfo {
    /// 1, 2, 3, ...
    pub version: u32,
    pub parent: Option<u32>,
    /// Unix seconds UTC.
    pub created_at: i64,
    /// The machine whose scan this version was made or updated from.
    pub source_machine_id: Option<String>,
    pub note: Option<String>,
}

/// The `machine.id` a Golden version's manifest carries, e.g. `golden@2`.
pub fn golden_machine_id(version: u32) -> String {
    format!("golden@{version}")
}

/// The display name of a Golden version, e.g. "Golden Rig v2".
pub fn golden_name(version: u32) -> String {
    format!("Golden Rig v{version}")
}
