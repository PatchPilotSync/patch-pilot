use std::collections::BTreeSet;
use std::path::PathBuf;

use patchpilot_core::{Manifest, Os, TargetProfile};
use patchpilot_parity::{
    ReportInfo, StepStatus, plan_progress, plan_still_covers, setup_progress, setup_report,
};

fn load(name: &str) -> Manifest {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/parity/plan")
        .join(name);
    Manifest::from_json(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// The laptop after installing these Primary Rig products.
fn with_golden(laptop: &Manifest, golden: &Manifest, products: &[&str]) -> Manifest {
    let mut m = laptop.clone();
    for id in products {
        m.items
            .retain(|i| i.product_id.as_ref().is_none_or(|p| p.0 != *id));
        for item in golden
            .items
            .iter()
            .filter(|i| i.product_id.as_ref().is_some_and(|p| p.0 == *id))
        {
            m.items.push(item.clone());
        }
        if let Some(p) = golden.products.iter().find(|p| p.canonical_id.0 == *id)
            && !m.products.iter().any(|q| q.canonical_id == p.canonical_id)
        {
            m.products.push(p.clone());
        }
    }
    m
}

fn status(p: &patchpilot_parity::SetupProgress) -> Vec<(&str, StepStatus)> {
    p.steps.iter().map(|s| (s.key.as_str(), s.status)).collect()
}

#[test]
fn progress_follows_rescans() {
    let golden = load("golden.json");
    let laptop = load("laptop.json");
    let profile = TargetProfile::default();
    let none = BTreeSet::new();

    let start = setup_progress(&golden, &profile, &laptop, &laptop, &none);
    assert_eq!(start.total, 9, "6 plan steps and 3 final checks");
    assert_eq!(start.done, 0);
    assert!(start.steps.iter().all(|s| s.status == StepStatus::Todo));
    assert_eq!(start.score_before, start.score_now);
    assert!(!start.complete);
    assert_eq!(start.steps[0].group, "Foundation");
    assert_eq!(start.steps[8].group, "Final checks");
    assert!(!start.steps[8].automatic);

    // iLok, Native Access and the Kontakt update are in.
    let later = with_golden(
        &laptop,
        &golden,
        &[
            "app/ilok-license-manager",
            "app/native-access",
            "native-instruments/kontakt-7",
        ],
    );
    let mut ticked = BTreeSet::from(["check:authorize".to_owned()]);
    let p = setup_progress(&golden, &profile, &laptop, &later, &ticked);
    assert_eq!(
        status(&p)[..4],
        [
            ("step:1", StepStatus::Done),
            ("step:2", StepStatus::Done),
            ("step:3", StepStatus::Partly),
            ("step:4", StepStatus::Todo),
        ]
    );
    let nks = &p.steps[2];
    assert_eq!(nks.open, ["Massive X 1.4.2"]);
    assert_eq!(nks.finished, ["Kontakt 7 7.10.1 (you have 7.9.0)"]);
    assert_eq!(p.steps[6].status, StepStatus::Done, "authorize was ticked");
    assert_eq!(p.done, 3);
    assert!(p.score_now > p.score_before);

    // The user skips the Waves step; it's reported as marked by hand.
    ticked.insert("step:4".to_owned());
    let p = setup_progress(&golden, &profile, &laptop, &later, &ticked);
    assert_eq!(p.steps[3].status, StepStatus::Skipped);

    // Everything installed and every check ticked: complete.
    let all: Vec<&str> = golden
        .items
        .iter()
        .filter_map(|i| i.product_id.as_ref().map(|p| p.0.as_str()))
        .collect();
    let done = with_golden(&laptop, &golden, &all);
    ticked.extend(["check:libraries", "check:open-daws"].map(str::to_owned));
    let p = setup_progress(&golden, &profile, &laptop, &done, &ticked);
    assert_eq!(p.score_now, 100);
    assert!(
        p.steps.iter().all(|s| s.status == StepStatus::Done),
        "{:?}",
        status(&p)
    );
    assert!(p.complete);

    let report = setup_report(
        &ReportInfo {
            machine: "New Laptop",
            os: Os::Windows,
            profile: None,
            golden_version: 2,
            started_at: 1_792_778_640,
            finished_at: Some(1_792_778_640 + 3900),
            now: 1_792_778_640 + 4000,
        },
        &p,
    );
    assert!(
        report.starts_with("# Setup report: New Laptop\n"),
        "{report}"
    );
    assert!(report.contains("Windows · Whole Primary Rig · compared with Primary Rig v2"));
    assert!(report.contains("- Took: 1 h 5 min"));
    assert!(report.contains(&format!("- Parity score: {} → 100", p.score_before)));
    assert!(report.contains("- Steps done: 9 of 9"));
    assert!(report.contains("  - Kontakt 7 7.10.1 (you have 7.9.0)"));
    assert!(!report.contains("Still to do"));
}

#[test]
fn report_lists_open_work() {
    let golden = load("golden.json");
    let laptop = load("laptop.json");
    let p = setup_progress(
        &golden,
        &TargetProfile::default(),
        &laptop,
        &laptop,
        &BTreeSet::new(),
    );
    let report = setup_report(
        &ReportInfo {
            machine: "New Laptop",
            os: Os::Macos,
            profile: Some("Travel"),
            golden_version: 1,
            started_at: 0,
            finished_at: None,
            now: 600,
        },
        &p,
    );
    assert!(report.contains("macOS · Travel ·"));
    assert!(report.contains("- Not finished yet (report made 1970-01-01 00:10 UTC)"));
    assert!(
        report.contains(
            "## Still to do\n\n- Install iLok License Manager\n  - iLok License Manager 5.9.0\n"
        ),
        "{report}"
    );
    assert!(!report.contains("## Done"));
    // Acme Verb is here as a VST3 without the Primary Rig's AAX: no step asks
    // for it, the report says so.
    assert!(
        !p.steps
            .iter()
            .any(|s| s.open.iter().any(|l| l.contains("Acme Verb")))
    );
    assert!(
        report.contains(
            "## Installed in other formats\n\n- Acme Verb: VST3 here, Primary Rig also has AAX\n"
        ),
        "{report}"
    );
}

#[test]
fn up_to_speed_crosses_off_what_a_rescan_finds() {
    let golden = load("golden.json");
    let laptop = load("laptop.json");
    let profile = TargetProfile::default();
    let none = BTreeSet::new();

    let start = plan_progress(&golden, &profile, &laptop, &laptop, &none);
    assert_eq!((start.done, start.total), (0, 6), "no final checks");
    assert!(start.steps.iter().all(|s| s.automatic));

    // iLok and Native Access got installed: their steps stay, ticked off.
    let later = with_golden(
        &laptop,
        &golden,
        &["app/ilok-license-manager", "app/native-access"],
    );
    let p = plan_progress(&golden, &profile, &laptop, &later, &none);
    assert_eq!(p.total, 6);
    assert_eq!(
        status(&p)[..2],
        [("step:1", StepStatus::Done), ("step:2", StepStatus::Done)]
    );
    assert_eq!(p.done, 2);
    assert!(!p.complete);
    assert!(plan_still_covers(&golden, &profile, &laptop, &later));

    // Pro-Q 3 was in parity and gets removed: the old plan doesn't cover it.
    let mut lost = later.clone();
    lost.items.retain(|i| {
        i.product_id
            .as_ref()
            .is_none_or(|p| p.0 != "fabfilter/pro-q-3")
    });
    assert!(!plan_still_covers(&golden, &profile, &laptop, &lost));

    let all: Vec<&str> = golden
        .items
        .iter()
        .filter_map(|i| i.product_id.as_ref().map(|p| p.0.as_str()))
        .collect();
    let done = with_golden(&laptop, &golden, &all);
    let p = plan_progress(&golden, &profile, &laptop, &done, &none);
    assert!(p.complete, "{:?}", status(&p));
}

#[test]
fn an_older_app_found_later_reads_as_update() {
    let golden = load("golden.json");
    let laptop = load("laptop.json");
    let profile = TargetProfile::default();
    let none = BTreeSet::new();

    // The plan asked to install iLok License Manager; a rescan finds an old one.
    let mut later = with_golden(&laptop, &golden, &["app/ilok-license-manager"]);
    for i in later.items.iter_mut().filter(|i| {
        i.product_id
            .as_ref()
            .is_some_and(|p| p.0 == "app/ilok-license-manager")
    }) {
        i.version = Some("5.1.0".into());
    }
    assert!(plan_still_covers(&golden, &profile, &laptop, &later));
    let p = plan_progress(&golden, &profile, &laptop, &later, &none);
    let step = &p.steps[0];
    assert_eq!(step.title, "Update iLok License Manager");
    assert_eq!(step.status, StepStatus::Todo);
    assert_eq!(step.open, ["iLok License Manager 5.9.0 (you have 5.1.0)"]);
}
