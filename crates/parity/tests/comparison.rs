use std::path::PathBuf;

use patchpilot_core::{Manifest, PluginFormat, TargetProfile};
use patchpilot_parity::{ParityState, compare};

fn load(rel: &str) -> Manifest {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(rel);
    Manifest::from_json(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn rows_carry_both_sides_and_vendor() {
    let golden = load("parity/diff/golden-studio-win.json");
    let laptop = load("parity/diff/laptop-mac.json");
    let c = compare(&golden, &TargetProfile::default(), &laptop);

    assert!(c.score < 100);
    assert!(!c.reasons.is_empty());
    assert_eq!(c.unresolved, 1, "Mystery Synth has no product_id");
    assert_eq!(c.rows[0].state, ParityState::Missing, "most urgent first");

    let verb = c.rows.iter().find(|r| r.name == "Acme Verb").unwrap();
    assert_eq!(verb.state, ParityState::Incomplete);
    assert_eq!(verb.missing_formats, [PluginFormat::Aax]);
    assert_eq!(verb.vendor, "Acme Audio");
    let formats = |items: &[patchpilot_parity::ItemSide]| {
        let mut f: Vec<_> = items.iter().filter_map(|i| i.format).collect();
        f.sort_by_key(|f| format!("{f:?}"));
        f
    };
    assert_eq!(
        formats(&verb.golden_items),
        [PluginFormat::Aax, PluginFormat::Vst3]
    );
    assert_eq!(
        formats(&verb.machine_items),
        [PluginFormat::Au, PluginFormat::Vst3]
    );

    let tuner = c.rows.iter().find(|r| r.name == "Acme Tuner").unwrap();
    assert_eq!(tuner.state, ParityState::Extra);
    assert!(tuner.golden_items.is_empty());

    let json = serde_json::to_value(&c).unwrap();
    assert_eq!(json["rows"][0]["state"], "missing");
    assert!(json["reasons"][0]["text"].is_string());
}

#[test]
fn profile_and_templates_feed_the_comparison() {
    let golden = load("parity/score/golden.json");
    let laptop = load("parity/score/laptop.json");
    let travel: TargetProfile = serde_json::from_str(
        &std::fs::read_to_string(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../fixtures/parity/score/travel-profile.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(compare(&golden, &travel, &laptop).score, 100);
    assert!(compare(&golden, &TargetProfile::default(), &laptop).score < 100);

    let c = compare(
        &load("parity/readiness/golden.json"),
        &TargetProfile::default(),
        &load("parity/readiness/laptop.json"),
    );
    let comp = c.rows.iter().find(|r| r.name == "Acme Comp").unwrap();
    assert_eq!(comp.state, ParityState::Missing);
    assert!(comp.needed_by_templates);
    assert_eq!(c.templates.len(), 1);
    assert_eq!(
        c.templates[0].missing,
        std::slice::from_ref(&comp.product_id)
    );
}
