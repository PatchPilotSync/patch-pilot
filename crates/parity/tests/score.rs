use std::collections::BTreeSet;
use std::path::PathBuf;

use patchpilot_core::{Manifest, ProductId, TargetProfile};
use patchpilot_parity::{ScoreWeights, apply_profile, score_machine};

fn read(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/parity/score")
        .join(name);
    std::fs::read_to_string(path).unwrap()
}

fn manifest(name: &str) -> Manifest {
    Manifest::from_json(&read(name)).unwrap()
}

fn travel() -> TargetProfile {
    serde_json::from_str(&read("travel-profile.json")).unwrap()
}

fn no_deps() -> BTreeSet<ProductId> {
    BTreeSet::new()
}

#[test]
fn travel_profile_excluding_aax_scores_100_when_only_aax_differs() {
    let s = score_machine(
        &manifest("golden.json"),
        &travel(),
        &manifest("laptop.json"),
        &no_deps(),
        &ScoreWeights::default(),
    );
    assert_eq!(s.score, 100, "{:?}", s.reasons);
    assert!(s.reasons.is_empty());
}

#[test]
fn without_a_profile_missing_aax_and_library_cost_points_with_reasons() {
    let s = score_machine(
        &manifest("golden.json"),
        &TargetProfile::default(),
        &manifest("laptop.json"),
        &no_deps(),
        &ScoreWeights::default(),
    );
    assert!(s.score < 100);
    let texts: Vec<_> = s.reasons.iter().map(|r| r.text.as_str()).collect();
    assert!(texts.contains(&"2 plug-ins are missing AAX"), "{texts:?}");
    assert!(texts.contains(&"1 folder is missing"), "{texts:?}");
    // Biggest cost first.
    assert!(s.reasons.windows(2).all(|w| w[0].points >= w[1].points));
    let lost: f64 = s.reasons.iter().map(|r| r.points).sum();
    assert!((100.0 - lost - f64::from(s.score)).abs() < 1.0);
}

#[test]
fn template_dependencies_weigh_more() {
    let golden = manifest("golden.json");
    let laptop = {
        let mut m = manifest("laptop.json");
        m.items
            .retain(|i| i.product_id.as_ref().is_none_or(|p| p.0 != "acme-comp"));
        m
    };
    let plain = score_machine(
        &golden,
        &travel(),
        &laptop,
        &no_deps(),
        &ScoreWeights::default(),
    );
    let deps: BTreeSet<ProductId> = [ProductId::from("acme-comp")].into();
    let needed = score_machine(&golden, &travel(), &laptop, &deps, &ScoreWeights::default());
    assert!(
        needed.score < plain.score,
        "{} vs {}",
        needed.score,
        plain.score
    );
    assert_eq!(
        needed.reasons[0].text,
        "1 plug-in your templates need is missing"
    );
}

#[test]
fn apply_profile_drops_excluded_items_and_orphaned_products() {
    let target = apply_profile(&manifest("golden.json"), &travel());
    assert!(
        target
            .items
            .iter()
            .all(|i| i.format != Some(patchpilot_core::PluginFormat::Aax))
    );
    assert!(target.product(&"big-strings".into()).is_none());
    assert!(target.product(&"acme-verb".into()).is_some());
}

#[test]
fn empty_target_scores_100() {
    let golden = manifest("golden.json");
    let mut empty = golden.clone();
    empty.items.clear();
    let s = score_machine(
        &empty,
        &TargetProfile::default(),
        &golden,
        &no_deps(),
        &ScoreWeights::default(),
    );
    assert_eq!(s.score, 100);
}
