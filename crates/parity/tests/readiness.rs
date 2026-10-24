use std::collections::BTreeSet;
use std::path::PathBuf;

use patchpilot_core::{
    ItemKind, MachineInfo, Manifest, Os, PathMap, PluginFormat, PortablePath, Product, ProductId,
    TargetProfile,
};
use patchpilot_parity::reaper::{PluginRef, REAPER_RESOURCES, template_items};
use patchpilot_parity::{
    ParityState, ScoreWeights, align_products, required_products, score_machine, template_readiness,
};

fn resource_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/parity/reaper/REAPER")
}

fn slug(s: &str) -> String {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join("-")
}

/// Stand-in for `patchpilot_scanner::product_id`; stock JSFX are skipped.
fn product_of(r: &PluginRef) -> Option<ProductId> {
    (r.format != PluginFormat::Jsfx).then(|| {
        let vendor = r.vendor.as_deref().map_or("unknown".into(), slug);
        ProductId(format!("{vendor}/{}", slug(&r.name)))
    })
}

/// A Windows machine with the fixture templates and these Acme plug-ins.
fn machine(plugins: &[&str]) -> Manifest {
    let info = MachineInfo {
        id: "studio".into(),
        name: "Studio".into(),
        os: Os::Windows,
        arch: patchpilot_core::Arch::X86_64,
        os_version: None,
        path_map: PathMap::new(Os::Windows),
    };
    let mut m = Manifest::new(info, 0);
    let (products, items) = template_items(&resource_dir(), Os::Windows, product_of);
    m.products = products;
    m.items = items;
    for name in plugins {
        let id = ProductId(format!("acme-audio/{}", slug(name)));
        m.products.push(Product {
            canonical_id: id.clone(),
            vendor: "Acme Audio".into(),
            name: (*name).into(),
        });
        let mut item = patchpilot_core::Item::plugin(
            PluginFormat::Vst3,
            Os::Windows,
            PortablePath::logical("VST3_SYSTEM", format!("{name}.vst3")),
        );
        item.product_id = Some(id);
        item.version = Some("1.0".into());
        m.items.push(item);
    }
    m
}

const ALL: &[&str] = &[
    "Acme Verb",
    "Acme Comp",
    "Acme Delay",
    "Acme Synth",
    "Acme Piano",
    "Acme EQ (Mono)",
];

#[test]
fn templates_become_portable_items_with_requirements() {
    let (products, items) = template_items(&resource_dir(), Os::Macos, product_of);
    let names: Vec<_> = products.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "ColorThemes/Studio Dark.ReaperTheme",
            "FXChains/Bus Glue.RfxChain",
            "ProjectTemplates/Mix Template.RPP",
            "Scripts/Tools/Nudge.lua",
            "TrackTemplates/Vocals/Lead Vocal.RTrackTemplate",
        ]
    );
    // Scripts and themes are tracked (#44) but need no plug-ins; compiled
    // code, hidden folders and `__pycache__` under Scripts/ are skipped.
    assert_eq!(items.len(), 5);
    let ids: Vec<_> = products.iter().map(|p| p.canonical_id.0.as_str()).collect();
    assert!(
        ids.contains(&"reaper-script/scripts-tools-nudge-lua"),
        "{ids:?}"
    );
    assert!(
        ids.contains(&"reaper-theme/colorthemes-studio-dark-reapertheme"),
        "{ids:?}"
    );
    assert_eq!(items.iter().filter(|i| !i.requires.is_empty()).count(), 3);
    let lead = items
        .iter()
        .find(|i| {
            i.path
                == PortablePath::logical(
                    REAPER_RESOURCES,
                    "TrackTemplates/Vocals/Lead Vocal.RTrackTemplate",
                )
        })
        .expect("lead vocal template");
    assert_eq!(lead.kind, ItemKind::DawAsset);
    assert!(lead.platform_portable && lead.content_hash.is_some());
    let req: Vec<_> = lead.requires.iter().map(|p| p.0.as_str()).collect();
    // JSFX are left out by `product_of`; sorted and unique.
    assert_eq!(req, ["acme-audio/acme-comp", "acme-audio/acme-verb"]);
}

#[test]
fn readiness_names_the_missing_plugin() {
    let all_but_comp: Vec<_> = ALL.iter().copied().filter(|n| *n != "Acme Comp").collect();
    let m = machine(&all_but_comp);
    let ready = template_readiness(&m);
    assert_eq!(ready.len(), 3);
    let lead = ready
        .iter()
        .find(|t| t.name.ends_with("Lead Vocal.RTrackTemplate"))
        .unwrap();
    assert!(!lead.is_ready());
    assert_eq!(lead.missing, [ProductId::from("acme-audio/acme-comp")]);
    assert!(
        ready.iter().filter(|t| t.is_ready()).count() == 2,
        "{ready:?}"
    );
    assert!(
        template_readiness(&machine(ALL))
            .iter()
            .all(|t| t.is_ready())
    );
}

#[test]
fn score_reason_says_templates_need_it() {
    let golden = machine(ALL);
    let all_but_comp: Vec<_> = ALL.iter().copied().filter(|n| *n != "Acme Comp").collect();
    let laptop = machine(&all_but_comp);
    let deps = required_products(&[&golden, &laptop]);
    assert!(deps.contains(&ProductId::from("acme-audio/acme-comp")));

    let with = score_machine(
        &golden,
        &TargetProfile::default(),
        &laptop,
        &deps,
        &ScoreWeights::default(),
    );
    let without = score_machine(
        &golden,
        &TargetProfile::default(),
        &laptop,
        &BTreeSet::new(),
        &ScoreWeights::default(),
    );
    assert!(
        with.score < without.score,
        "{} vs {}",
        with.score,
        without.score
    );
    assert_eq!(
        with.reasons[0].text,
        "1 plug-in your templates need is missing"
    );
    assert_eq!(
        with.reasons[0].products,
        [ProductId::from("acme-audio/acme-comp")]
    );
    // Same templates on both machines: they're in parity.
    let lead = with
        .diff
        .products
        .iter()
        .find(|p| p.name.ends_with("Lead Vocal.RTrackTemplate"))
        .unwrap();
    assert_eq!(lead.state, ParityState::InParity);
}

#[test]
fn unknown_vendor_requirements_align() {
    let mut golden = machine(ALL);
    let mut laptop = machine(ALL);
    // A template written by an older Reaper with no vendor in the name.
    laptop.items[0].requires = vec![ProductId::from("unknown/acme-comp")];
    align_products(&mut golden, &mut laptop);
    assert_eq!(
        laptop.items[0].requires,
        [ProductId::from("acme-audio/acme-comp")]
    );
}
