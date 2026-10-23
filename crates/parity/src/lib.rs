//! Identity resolution, manifest diff, parity scoring and plan generation.

pub mod reaper;

mod diff;
mod identity;
mod score;
mod version;

pub use diff::{ManifestDiff, ParityState, ProductDiff, diff_manifests, format_available_on};
pub use identity::align_products;
pub use score::{ParityScore, Reason, ScoreWeights, apply_profile, score_machine};
pub use version::{Version, VersionOrder, compare_versions};
