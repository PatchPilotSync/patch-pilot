//! Template readiness (#37, strategy section 16 step 3): does a machine have
//! every plug-in its templates, racks and projects use?

use std::collections::BTreeSet;

use patchpilot_core::{Manifest, ProductId};

use crate::reaper_ext::is_extension;

/// One template (or rack, or project) on a machine.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TemplateReadiness {
    pub product_id: ProductId,
    /// Display name, e.g. `TrackTemplates/Vocals/Lead Vocal.RTrackTemplate`.
    pub name: String,
    /// Where it is on that machine, as the machine's OS spells it. `None` when
    /// its root isn't mapped there. Only openable on the machine itself.
    pub path: Option<String>,
    /// Required products the machine has no item for. Empty = ready.
    pub missing: Vec<ProductId>,
}

impl TemplateReadiness {
    pub fn is_ready(&self) -> bool {
        self.missing.is_empty()
    }
}

/// Readiness of every item on `machine` that requires other products,
/// in manifest order. Run [`crate::align_products`] first so `unknown/`
/// references match. The Reaper extensions scripts call are left out:
/// the extensions check reports those (decision #104).
pub fn template_readiness(machine: &Manifest) -> Vec<TemplateReadiness> {
    let have: BTreeSet<&ProductId> = machine
        .items
        .iter()
        .filter_map(|i| i.product_id.as_ref())
        .collect();
    machine
        .items
        .iter()
        .filter(|i| i.requires.iter().any(|r| !is_extension(r)))
        .filter_map(|i| {
            let id = i.product_id.clone()?;
            let name = machine
                .product(&id)
                .map_or_else(|| id.to_string(), |p| p.name.clone());
            Some(TemplateReadiness {
                missing: i
                    .requires
                    .iter()
                    .filter(|r| !have.contains(r) && !is_extension(r))
                    .cloned()
                    .collect(),
                product_id: id,
                name,
                path: machine.machine.path_map.resolve(&i.path),
            })
        })
        .collect()
}

/// Every product something in these manifests requires: the set
/// [`crate::score_machine`] weighs up as template dependencies.
pub fn required_products(manifests: &[&Manifest]) -> BTreeSet<ProductId> {
    manifests
        .iter()
        .flat_map(|m| &m.items)
        .flat_map(|i| i.requires.iter().cloned())
        .filter(|r| !is_extension(r))
        .collect()
}
