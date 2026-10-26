//! One machine against the Primary Rig (#41): score, reasons, per-product rows
//! with both sides' items, and template readiness, for the comparison view.

use std::collections::HashMap;

use patchpilot_core::{
    Confidence, Item, ItemKind, Manifest, Os, PluginFormat, PortablePath, ProductId, TargetProfile,
};
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
    /// The target's items for this product (Primary Rig after the profile).
    pub golden_items: Vec<ItemSide>,
    pub machine_items: Vec<ItemSide>,
    /// Other files this one needs beside it to work, directly or through
    /// others (decision #62), as paths below its root. Empty for most rows.
    pub works_with: Vec<String>,
    /// Those of `works_with` the machine has no copy of.
    pub works_with_missing: Vec<String>,
    /// The Primary Rig has it, but the machine's profile or an Ignore leaves
    /// it out, so it is not a suggestion for the Primary Rig.
    pub left_out: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ItemSide {
    pub format: Option<PluginFormat>,
    pub platform: Os,
    pub version: Option<String>,
    /// Bytes, where the scan measured it (preset folders).
    pub size: Option<u64>,
    /// Where it lives, for a preset folder, e.g. `Documents\Kilohearts`.
    pub location: Option<String>,
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
    let target_files = files_by_path(&target);
    let machine_files = files_by_path(&machine);

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
                        size: i.size.filter(|_| crate::presets::is_preset_item(i)),
                        location: product.and_then(|p| crate::presets::location(p, i.platform)),
                    })
                    .collect()
            };
            let items: Vec<_> = [&target, &machine]
                .into_iter()
                .flat_map(|m| m.items.iter())
                .filter(|i| i.product_id.as_ref() == Some(&d.product_id))
                .collect();
            let (works_with, works_with_missing) =
                works_with(&items, &target_files, &machine_files);
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
                works_with,
                works_with_missing,
                left_out: target.product(&d.product_id).is_none()
                    && golden.product(&d.product_id).is_some(),
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

/// Items at a logical path, by `root/rel` lowercased.
type Files<'a> = HashMap<String, &'a Item>;

fn files_by_path(m: &Manifest) -> Files<'_> {
    m.items
        .iter()
        .filter_map(|i| match &i.path {
            PortablePath::Logical { root, rel } => {
                Some((format!("{root}/{rel}").to_lowercase(), i))
            }
            _ => None,
        })
        .collect()
}

/// What the first of `items` (target's first) that needs other files needs,
/// through others too, and which of those `machine` lacks.
fn works_with(items: &[&Item], target: &Files, machine: &Files) -> (Vec<String>, Vec<String>) {
    let Some(item) = items.iter().find(|i| !i.needs_files.is_empty()) else {
        return (Vec::new(), Vec::new());
    };
    let PortablePath::Logical { root, rel } = &item.path else {
        return (Vec::new(), Vec::new());
    };
    let lookup = |rel: &str| {
        let key = format!("{root}/{rel}").to_lowercase();
        target.get(&key).or_else(|| machine.get(&key)).copied()
    };
    let mut all = crate::reaper_links::with_needs([rel.as_str()], |r| {
        lookup(r).map(|i| i.needs_files.as_slice())
    });
    all.retain(|r| !r.eq_ignore_ascii_case(rel));
    let all: Vec<String> = all.into_iter().collect();
    let missing = all
        .iter()
        .filter(|r| !machine.contains_key(&format!("{root}/{r}").to_lowercase()))
        .cloned()
        .collect();
    (all, missing)
}

/// Low < Medium < High.
pub(crate) fn confidence_rank(c: Confidence) -> u8 {
    match c {
        Confidence::Low => 0,
        Confidence::Medium => 1,
        Confidence::High => 2,
    }
}
