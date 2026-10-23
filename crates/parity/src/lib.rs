//! Identity resolution, manifest diff, parity scoring and plan generation.

mod diff;
mod version;

pub use diff::{ManifestDiff, ParityState, ProductDiff, diff_manifests, format_available_on};
pub use version::{Version, VersionOrder, compare_versions};
