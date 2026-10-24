use std::path::PathBuf;

use patchpilot_core::{Manifest, TargetProfile};
use patchpilot_parity::{Phase, align_products, up_to_speed};

fn load(name: &str) -> Manifest {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/parity/plan")
        .join(name);
    Manifest::from_json(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn plan_for(profile: &TargetProfile) -> patchpilot_parity::Plan {
    let mut golden = load("golden.json");
    let mut laptop = load("laptop.json");
    align_products(&mut golden, &mut laptop);
    up_to_speed(&golden, profile, &laptop)
}

#[test]
fn stable_ordered_plan() {
    let plan = plan_for(&TargetProfile::default());
    let rows: Vec<_> = plan
        .steps
        .iter()
        .map(|s| (s.number, s.phase, s.title.as_str(), s.after.clone()))
        .collect();
    assert_eq!(
        rows,
        [
            (1, Phase::Foundation, "Install iLok License Manager", vec![]),
            (2, Phase::Foundation, "Install Native Access", vec![1]),
            (
                3,
                Phase::VendorHandoff,
                "Open Native Access and install 1 and update 1",
                vec![1, 2]
            ),
            (
                4,
                Phase::VendorHandoff,
                "Open Waves Central and install 1",
                vec![1]
            ),
            (
                5,
                Phase::Manual,
                "Install from Acme Audio's installer or website: install 2",
                vec![1]
            ),
            (
                6,
                Phase::Manual,
                "Copy 1 DAW file from the Golden machine (file sync comes later)",
                vec![]
            ),
        ]
    );
    assert_eq!(
        plan.steps[2].details,
        ["Kontakt 7 7.10.1 (you have 7.9.0)", "Massive X 1.4.2"]
    );
    assert_eq!(
        plan.steps[4].details,
        ["Acme Gate 1.2", "Acme Verb: add AAX"]
    );
    // Steps that use a vendor app name it, so the app can open it.
    let apps: Vec<_> = plan.steps.iter().map(|s| s.app.as_deref()).collect();
    assert_eq!(
        apps,
        [
            Some("ilok-license-manager"),
            Some("native-access"),
            Some("native-access"),
            Some("waves-central"),
            None,
            None,
        ]
    );
    // Same input, same plan.
    assert_eq!(plan, plan_for(&TargetProfile::default()));
}

#[test]
fn never_suggests_copying_plugins() {
    let plan = plan_for(&TargetProfile::default());
    for s in &plan.steps {
        let plugin_step = s
            .products
            .iter()
            .any(|p| !p.0.starts_with("reaper-template/"));
        if plugin_step {
            let text = format!("{} {}", s.title, s.details.join(" ")).to_lowercase();
            assert!(!text.contains("copy"), "{s:?}");
        }
    }
}

#[test]
fn profile_drops_steps() {
    // Travel without AAX: Acme Verb is complete, only Acme Gate is left.
    let travel: TargetProfile = serde_json::from_str(
        r#"{"id":"travel","name":"Travel","rules":[{"action":"exclude","match":{"format":"aax"}}]}"#,
    )
    .unwrap();
    let plan = plan_for(&travel);
    let acme = plan
        .steps
        .iter()
        .find(|s| s.title.contains("Acme Audio"))
        .unwrap();
    assert_eq!(acme.details, ["Acme Gate 1.2"]);

    let golden = load("golden.json");
    assert!(
        up_to_speed(&golden, &TargetProfile::default(), &golden)
            .steps
            .is_empty()
    );
}
