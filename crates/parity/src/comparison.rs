//! One machine against the Golden Rig (#41): score, reasons, per-product rows
//! with both sides' items, and template readiness, for the comparison view.

use patchpilot_core::{Confidence, ItemKind, Manifest, Os, PluginFormat, ProductId, TargetProfile};
use serde::Serialize;

use crate::{
    ParityState, Reason, ScoreWeights, TemplateReadiness, align_products, apply_profile,
    required_products, score_machine, template_readiness,
};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Comparison {
    /// 0–100 against the target profile.
    pub score: u8,
    /// Biggest cost first.
    pub reasons: Vec<Reason>,
    /// Most urgent state first, then name.
    pub rows: Vec<ComparisonRow>,
    /// Templates on the machine and the plug-ins each one lacks.
    pub templates: Vec<TemplateReadiness>,
    /// Machine items that couldn't be tied to a product.
    pub unresolved: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ComparisonRow {
    pub product_id: ProductId,
    pub name: String,
    /// Empty when unknown.
    pub vendor: String,
    pub kind: ItemKind,
    pub state: ParityState,
    pub golden_version: Option<String>,
    pub machine_version: Option<String>,
    pub missing_formats: Vec<PluginFormat>,
    /// Weakest identity confidence among the product's items on both sides.
    pub confidence: Confidence,
    /// A template on either side uses it, so it weighs more in the score.
    pub needed_by_templates: bool,
    /// The target's items for this product (Golden after the profile).
    pub golden_items: Vec<ItemSide>,
    pub machine_items: Vec<ItemSide>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ItemSide {
    pub format: Option<PluginFormat>,
    pub platform: Os,
    pub version: Option<String>,
}

/// Compare `machine` with `golden` through `profile`. Aligns `unknown/`
/// product IDs first, so callers pass manifests as stored.
pub fn compare(golden: &Manifest, profile: &TargetProfile, machine: &Manifest) -> Comparison {
    let (mut golden, mut machine) = (golden.clone(), machine.clone());
    align_products(&mut golden, &mut machine);
    let needed = required_products(&[&golden, &machine]);
    let score = score_machine(
        &golden,
        profile,
        &machine,
        &needed,
        &ScoreWeights::default(),
    );
    let target = apply_profile(&golden, profile);

    let rows = score
        .diff
        .products
        .iter()
        .map(|d| {
            let product = target
                .product(&d.product_id)
                .or_else(|| machine.product(&d.product_id));
            let sides = |m: &Manifest| -> Vec<ItemSide> {
                m.items
                    .iter()
                    .filter(|i| i.product_id.as_ref() == Some(&d.product_id))
                    .map(|i| ItemSide {
                        format: i.format,
                        platform: i.platform,
                        version: i.version.clone(),
                    })
                    .collect()
            };
            let items: Vec<_> = [&target, &machine]
                .into_iter()
                .flat_map(|m| m.items.iter())
                .filter(|i| i.product_id.as_ref() == Some(&d.product_id))
                .collect();
            ComparisonRow {
                product_id: d.product_id.clone(),
                name: d.name.clone(),
                vendor: product.map(|p| p.vendor.clone()).unwrap_or_default(),
                kind: items.first().map_or(ItemKind::Plugin, |i| i.kind),
                state: d.state,
                golden_version: d.golden_version.clone(),
                machine_version: d.machine_version.clone(),
                missing_formats: d.missing_formats.clone(),
                confidence: items
                    .iter()
                    .map(|i| i.confidence)
                    .min_by_key(|c| confidence_rank(*c))
                    .unwrap_or_default(),
                needed_by_templates: needed.contains(&d.product_id),
                golden_items: sides(&target),
                machine_items: sides(&machine),
            }
        })
        .collect();

    Comparison {
        score: score.score,
        reasons: score.reasons,
        rows,
        templates: template_readiness(&machine),
        unresolved: score.diff.unresolved.len(),
    }
}

/// Low < Medium < High.
pub(crate) fn confidence_rank(c: Confidence) -> u8 {
    match c {
        Confidence::Low => 0,
        Confidence::Medium => 1,
        Confidence::High => 2,
    }
}
