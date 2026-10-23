//! Identity resolution, manifest diff, parity scoring and plan generation.

pub mod reaper;

mod diff;
mod golden;
mod identity;
mod plan;
mod readiness;
mod score;
mod version;

pub use diff::{ManifestDiff, ParityState, ProductDiff, diff_manifests, format_available_on};
pub use golden::{ProposeError, find_product, propose_to_golden};
pub use identity::align_products;
pub use plan::{Phase, Plan, Step, up_to_speed};
pub use readiness::{TemplateReadiness, required_products, template_readiness};
pub use score::{ParityScore, Reason, ScoreWeights, apply_profile, score_machine};
pub use version::{Version, VersionOrder, compare_versions};
