//! Parity score v1: one number per machine, always with reasons.
//!
//! Each Primary Rig product (after the target profile) carries a weight by kind,
//! boosted when a template depends on it. Its state earns a share of that
//! weight. The score is earned / possible, 0–100. Reasons group the lost
//! weight so the UI can say "92% · 3 plug-ins your templates need are missing".

use std::collections::{BTreeSet, HashMap};

use patchpilot_core::{ItemKind, Manifest, PluginFormat, ProductId, TargetProfile};

use crate::diff::{ManifestDiff, ParityState, ProductDiff, diff_manifests};

/// Tunable weights. [`Default`] is the v1 scheme.
#[derive(Debug, Clone, PartialEq)]
pub struct ScoreWeights {
    pub plugin: f64,
    pub license_tool: f64,
    pub vendor_app: f64,
    pub daw_asset: f64,
    pub config: f64,
    pub folder: f64,
    /// Multiplier for products a template, rack or project depends on.
    pub template_dependency: f64,
}

impl Default for ScoreWeights {
    fn default() -> Self {
        Self {
            plugin: 1.0,
            // Without the licence tool, every PACE plug-in fails to load.
            license_tool: 2.0,
            vendor_app: 0.5,
            daw_asset: 0.5,
            config: 0.5,
            folder: 0.25,
            template_dependency: 3.0,
        }
    }
}

impl ScoreWeights {
    fn for_kind(&self, kind: ItemKind) -> f64 {
        match kind {
            ItemKind::Plugin => self.plugin,
            ItemKind::LicenseTool => self.license_tool,
            ItemKind::VendorApp => self.vendor_app,
            ItemKind::DawAsset => self.daw_asset,
            ItemKind::Config => self.config,
            ItemKind::Folder => self.folder,
        }
    }
}

/// Share of a product's weight each state earns.
fn credit(state: ParityState) -> Option<f64> {
    use ParityState::*;
    Some(match state {
        InParity | NewerThanGolden => 1.0,
        Modified | VersionUnknown => 0.75,
        Outdated | Incomplete => 0.5,
        Missing => 0.0,
        // Not part of the target: neither helps nor hurts.
        Extra | NotApplicable => return None,
    })
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Reason {
    /// Plain-language summary, e.g. "2 plug-ins are missing".
    pub text: String,
    pub products: Vec<ProductId>,
    /// Score points this reason costs.
    pub points: f64,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ParityScore {
    /// 0–100, rounded down so 99.6% never shows as 100.
    pub score: u8,
    /// Biggest cost first.
    pub reasons: Vec<Reason>,
    pub diff: ManifestDiff,
}

/// Apply `profile` to `golden`, diff `machine` against it, and score.
///
/// `template_deps` are products that the machine's templates, racks or
/// projects need (from the readiness scanners); pass an empty set if unknown.
pub fn score_machine(
    golden: &Manifest,
    profile: &TargetProfile,
    machine: &Manifest,
    template_deps: &BTreeSet<ProductId>,
    weights: &ScoreWeights,
) -> ParityScore {
    let target = apply_profile(golden, profile);
    let diff = diff_manifests(&target, machine);

    let mut possible = 0.0;
    // (reason key, product, lost weight)
    let mut losses: Vec<(ReasonKey, &ProductDiff, f64)> = Vec::new();
    // Each product's kind is its first item's.
    let mut kinds: HashMap<&ProductId, ItemKind> = HashMap::new();
    for item in &target.items {
        if let Some(id) = &item.product_id {
            kinds.entry(id).or_insert(item.kind);
        }
    }
    for p in &diff.products {
        let Some(credit) = credit(p.state) else {
            continue;
        };
        let kind = kinds
            .get(&p.product_id)
            .copied()
            .unwrap_or(ItemKind::Plugin);
        let needed = template_deps.contains(&p.product_id);
        let mut w = weights.for_kind(kind);
        if needed {
            w *= weights.template_dependency;
        }
        possible += w;
        if credit < 1.0 {
            losses.push((ReasonKey::new(p, kind, needed), p, w * (1.0 - credit)));
        }
    }
    let lost: f64 = losses.iter().map(|(_, _, l)| l).sum();
    let score = if possible == 0.0 {
        100
    } else {
        // Clamp guards float noise; floor so a near-miss never shows 100.
        (((possible - lost) / possible) * 100.0)
            .clamp(0.0, 100.0)
            .floor() as u8
    };

    let mut reasons: Vec<Reason> = Vec::new();
    let mut keys: Vec<ReasonKey> = Vec::new();
    for (key, p, l) in losses {
        let points = if possible == 0.0 {
            0.0
        } else {
            l / possible * 100.0
        };
        match keys.iter().position(|k| *k == key) {
            Some(i) => {
                reasons[i].products.push(p.product_id.clone());
                reasons[i].points += points;
            }
            None => {
                keys.push(key);
                reasons.push(Reason {
                    text: String::new(),
                    products: vec![p.product_id.clone()],
                    points,
                });
            }
        }
    }
    for (r, k) in reasons.iter_mut().zip(&keys) {
        r.text = k.describe(r.products.len());
    }
    reasons.sort_by(|a, b| b.points.total_cmp(&a.points));

    ParityScore {
        score,
        reasons,
        diff,
    }
}

/// Primary Rig filtered through a target profile.
pub fn apply_profile(golden: &Manifest, profile: &TargetProfile) -> Manifest {
    let mut target = golden.clone();
    let products: HashMap<_, _> = golden
        .products
        .iter()
        .rev()
        .map(|p| (&p.canonical_id, p))
        .collect();
    target.items.retain(|item| {
        let product = item
            .product_id
            .as_ref()
            .and_then(|id| products.get(id).copied());
        profile.includes(item, product)
    });
    let kept: BTreeSet<_> = target
        .items
        .iter()
        .filter_map(|i| i.product_id.as_ref())
        .collect();
    target.products.retain(|p| kept.contains(&p.canonical_id));
    target
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReasonKey {
    state: ParityState,
    kind: ItemKind,
    needed_by_templates: bool,
    /// For `Incomplete`, the formats missing, so "missing AAX" groups together.
    formats: Vec<PluginFormat>,
}

impl ReasonKey {
    fn new(p: &ProductDiff, kind: ItemKind, needed: bool) -> Self {
        Self {
            state: p.state,
            kind,
            needed_by_templates: needed,
            formats: if p.state == ParityState::Incomplete {
                p.missing_formats.clone()
            } else {
                Vec::new()
            },
        }
    }

    fn describe(&self, n: usize) -> String {
        let noun = match (self.kind, n == 1) {
            (ItemKind::Plugin, true) => "plug-in",
            (ItemKind::Plugin, false) => "plug-ins",
            (ItemKind::LicenseTool, true) => "licensing tool",
            (ItemKind::LicenseTool, false) => "licensing tools",
            (ItemKind::VendorApp, true) => "vendor app",
            (ItemKind::VendorApp, false) => "vendor apps",
            (ItemKind::DawAsset, true) => "DAW file",
            (ItemKind::DawAsset, false) => "DAW files",
            (ItemKind::Config, true) => "config file",
            (ItemKind::Config, false) => "config files",
            (ItemKind::Folder, true) => "folder",
            (ItemKind::Folder, false) => "folders",
        };
        let subject = if self.needed_by_templates {
            format!("{n} {noun} your templates need")
        } else {
            format!("{n} {noun}")
        };
        let be = if n == 1 { "is" } else { "are" };
        let state = match self.state {
            ParityState::Missing => format!("{be} missing"),
            ParityState::Outdated => format!("{be} out of date"),
            ParityState::Modified => format!("{be} different from the Primary Rig"),
            ParityState::VersionUnknown => format!("{be} at a version we can't compare"),
            ParityState::Incomplete => {
                let formats: Vec<_> = self.formats.iter().map(|f| format_name(*f)).collect();
                format!("{be} missing {}", formats.join(" and "))
            }
            // Never lose points; listed for completeness.
            ParityState::NewerThanGolden
            | ParityState::Extra
            | ParityState::NotApplicable
            | ParityState::InParity => format!("{be} fine"),
        };
        format!("{subject} {state}")
    }
}

fn format_name(f: PluginFormat) -> &'static str {
    match f {
        PluginFormat::Vst2 => "VST2",
        PluginFormat::Vst3 => "VST3",
        PluginFormat::Au => "AU",
        PluginFormat::Aax => "AAX",
        PluginFormat::Clap => "CLAP",
        PluginFormat::Jsfx => "JSFX",
    }
}
