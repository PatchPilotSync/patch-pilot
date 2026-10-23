//! Shared domain types for Patch Pilot.
//!
//! A [`Manifest`] is what one machine has: [`Product`]s and the [`Item`]s that
//! belong to them. Manifests are metadata only, never file contents. Paths in a
//! manifest are [`PortablePath`]s, so the same manifest reads the same on
//! Windows and macOS once a machine's [`PathMap`] is applied.
//!
//! Changes here are reviewed by both founders (CLAUDE.md, Ownership).

mod manifest;
mod path;
mod profile;

pub use manifest::{
    Arch, Confidence, ContentHash, Item, ItemKind, MachineInfo, Manifest, ManifestError, Os,
    PluginFormat, Product, ProductId,
};
pub use path::{PathMap, PortablePath};
pub use profile::{Matcher, ProfileRule, RuleAction, TargetProfile};

/// Version of the manifest JSON format this build reads and writes.
///
/// Bump it for any change an older build could misread, and document the
/// change in `docs/manifest.md`.
pub const MANIFEST_SCHEMA_VERSION: u32 = 1;

/// JSON Schema for [`Manifest`], as checked in at `docs/manifest.schema.json`.
pub fn manifest_json_schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(Manifest)).expect("schema serializes")
}
