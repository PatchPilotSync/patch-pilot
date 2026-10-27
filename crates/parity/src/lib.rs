//! Identity resolution, manifest diff, parity scoring and plan generation.

pub mod ableton;
pub mod presets;
pub mod reapack;
pub mod reaper;
pub mod reaper_folders;
pub mod reaper_keys;
pub mod reaper_links;

mod ini;

mod comparison;
mod diff;
mod drift;
mod golden;
mod history;
mod identity;
mod inventory;
mod plan;
mod readiness;
mod report;
mod score;
mod setup;
mod updates;
mod version;

pub use comparison::{Comparison, ComparisonRow, ItemSide, compare};
pub use diff::{ManifestDiff, ParityState, ProductDiff, diff_manifests, format_available_on};
pub use drift::{
    Alert, Observed, Packages, Seen, SuggestionGroup, SuggestionUnit, alerts, gaps, suggestion_key,
    suggestion_summary, suggestion_unit, suggestions,
};
pub use golden::{ProposeError, find_product, propose_to_golden};
pub use history::{Change, ProductChange, plugin_changes};
pub use identity::align_products;
pub use inventory::{
    AppRow, FormatCount, Inventory, PresetRow, ProductRow, VendorGroup, inventory,
};
pub use plan::{Need, OtherFormats, Phase, Plan, Step, up_to_speed};
pub use readiness::{TemplateReadiness, required_products, template_readiness};
pub use report::{ReportOptions, ReportParity, RigReport, rig_report};
pub use score::{ParityScore, Reason, ScoreWeights, apply_profile, score_machine};
pub use setup::{
    ReportInfo, SetupProgress, SetupStep, StepStatus, plan_progress, plan_still_covers,
    setup_progress, setup_report,
};
pub use updates::{Elsewhere, UpdateAlert, UpdateSource, update_alerts};
pub use version::{Version, VersionOrder, compare_versions};
