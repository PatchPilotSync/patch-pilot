use std::path::PathBuf;

use patchpilot_core::{Manifest, Os, PluginFormat};
use patchpilot_parity::{
    ParityState, ProposeError, diff_manifests, find_product, propose_to_golden,
};

fn load(name: &str) -> Manifest {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/parity/diff")
        .join(name);
    Manifest::from_json(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn state(golden: &Manifest, machine: &Manifest, id: &str) -> ParityState {
    diff_manifests(golden, machine)
        .products
        .into_iter()
        .find(|p| p.product_id.0 == id)
        .unwrap_or_else(|| panic!("{id} not in diff"))
        .state
}

#[test]
fn propose_newer_and_extra_products_from_laptop() {
    let golden = load("golden-studio-win.json");
    let studio = golden.clone();
    let laptop = load("laptop-mac.json");
    assert_eq!(
        state(&golden, &laptop, "acme-delay"),
        ParityState::NewerThanGolden
    );
    assert_eq!(state(&golden, &laptop, "acme-tuner"), ParityState::Extra);

    // By ID and by name, any case.
    let selected = ["acme-delay".to_owned(), "acme TUNER".to_owned()];
    let next = propose_to_golden(&golden, &laptop, &selected).unwrap();

    assert_eq!(state(&next, &laptop, "acme-delay"), ParityState::InParity);
    assert_eq!(state(&next, &laptop, "acme-tuner"), ParityState::InParity);
    // Primary Rig is per platform (decision 17): the Mac slot moved to 1.5, the
    // Windows studio PC is still measured against Primary Rig's Windows 1.4.
    assert_eq!(state(&next, &studio, "acme-delay"), ParityState::InParity);
    // Unselected products are untouched.
    assert_eq!(state(&next, &laptop, "acme-comp"), ParityState::Outdated);
    assert!(next.product(&"acme-tuner".into()).is_some());

    // Primary Rig keeps its Windows VST3 next to the Mac one it took.
    let delay: Vec<_> = next
        .items
        .iter()
        .filter(|i| i.product_id.as_ref().is_some_and(|p| p.0 == "acme-delay"))
        .map(|i| (i.platform, i.version.as_deref()))
        .collect();
    assert_eq!(
        delay,
        [(Os::Windows, Some("1.4")), (Os::Macos, Some("1.5"))]
    );
    // The input Primary Rig is not modified.
    assert_eq!(golden, load("golden-studio-win.json"));
}

#[test]
fn propose_replaces_the_same_slot() {
    let golden = load("golden-studio-win.json");
    let mut newer = golden.clone();
    newer.machine.name = "Studio PC 2".into();
    for i in &mut newer.items {
        if i.product_id.as_ref().is_some_and(|p| p.0 == "acme-verb")
            && i.format == Some(PluginFormat::Vst3)
        {
            i.version = Some("2.2.0".into());
        }
    }
    let next = propose_to_golden(&golden, &newer, &["Acme Verb".into()]).unwrap();
    let verb: Vec<_> = next
        .items
        .iter()
        .filter(|i| i.product_id.as_ref().is_some_and(|p| p.0 == "acme-verb"))
        .map(|i| (i.format, i.version.as_deref()))
        .collect();
    assert_eq!(verb.len(), 2, "{verb:?}");
    assert!(verb.contains(&(Some(PluginFormat::Vst3), Some("2.2.0"))));
    assert!(verb.contains(&(Some(PluginFormat::Aax), Some("2.1"))));
    assert_eq!(next.items.len(), golden.items.len());
}

#[test]
fn propose_errors() {
    let golden = load("golden-studio-win.json");
    let laptop = load("laptop-mac.json");
    assert_eq!(
        propose_to_golden(&golden, &laptop, &[]),
        Err(ProposeError::NothingSelected)
    );
    // Acme Gate is only on the studio PC.
    assert!(matches!(
        propose_to_golden(&golden, &laptop, &["Acme Gate".into()]),
        Err(ProposeError::NotOnMachine { .. })
    ));
    assert_eq!(find_product(&laptop, "nope"), None);
}
