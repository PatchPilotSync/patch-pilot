//! Identity resolution, manifest diff, parity scoring and plan generation.

pub mod ableton;
pub mod reaper;

mod comparison;
mod diff;
mod golden;
mod history;
mod identity;
mod inventory;
mod plan;
mod readiness;
mod report;
mod score;
mod version;

pub use comparison::{Comparison, ComparisonRow, ItemSide, compare};
pub use diff::{ManifestDiff, ParityState, ProductDiff, diff_manifests, format_available_on};
pub use golden::{ProposeError, find_product, propose_to_golden};
pub use history::{Change, ProductChange, plugin_changes};
pub use identity::align_products;
pub use inventory::{AppRow, FormatCount, Inventory, ProductRow, VendorGroup, inventory};
pub use plan::{Phase, Plan, Step, up_to_speed};
pub use readiness::{TemplateReadiness, required_products, template_readiness};
pub use report::{ReportOptions, ReportParity, RigReport, rig_report};
pub use score::{ParityScore, Reason, ScoreWeights, apply_profile, score_machine};
pub use version::{Version, VersionOrder, compare_versions};
