use std::path::{Path, PathBuf};

use patchpilot_core::{ItemKind, Os, PluginFormat, PortablePath, ProductId};
use patchpilot_parity::ableton::*;
use patchpilot_parity::reaper::PluginRef;

/// `vendor/name`, or `unknown/name`, like the scanner's product IDs.
fn by_name(r: &PluginRef) -> Option<ProductId> {
    let vendor = r.vendor.as_deref().unwrap_or("unknown");
    Some(ProductId(format!("{vendor}/{}", r.name)))
}

fn fixture(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/parity/ableton")
        .join(rel)
}

fn rel_of(p: &PortablePath) -> (&str, &str) {
    match p {
        PortablePath::Logical { root, rel } => (root, rel),
        PortablePath::Absolute(_) => panic!("absolute path {p:?}"),
    }
}

#[test]
fn newest_release_prefs_folder_wins() {
    assert_eq!(
        latest_prefs_dir(&fixture("prefs-mac"), Os::Macos),
        Some(fixture("prefs-mac").join("Live 12.1.5"))
    );
    assert_eq!(
        latest_prefs_dir(&fixture("prefs-win"), Os::Windows),
        Some(fixture("prefs-win").join("Live 12.1.5").join("Preferences"))
    );
    assert_eq!(latest_prefs_dir(&fixture("missing"), Os::Macos), None);
}

#[test]
fn reads_user_library_location_from_library_cfg() {
    let cfg = std::fs::read_to_string(fixture("prefs-mac/Live 12.1.5/Library.cfg")).unwrap();
    assert_eq!(
        user_library_from_cfg(&cfg),
        Some(PathBuf::from("/Users/alice/Music/Ableton/User Library"))
    );
}

#[test]
fn shares_settings_files_and_nothing_else() {
    let lib = fixture("User Library");
    let prefs = fixture("prefs-mac/Live 12.1.5");
    let (products, items) = settings_items(Some(&lib), Some(&prefs), Os::Macos, by_name);

    let paths: Vec<(&str, &str)> = items.iter().map(|i| rel_of(&i.path)).collect();
    assert_eq!(
        paths,
        [
            (
                ABLETON_USER_LIBRARY,
                "Ableton Folder Info/12/Collections.xmp"
            ),
            (ABLETON_USER_LIBRARY, "Clips/Bassline.alc"),
            (ABLETON_USER_LIBRARY, "Defaults/Audio Effects/EQ Eight.adv"),
            (
                ABLETON_USER_LIBRARY,
                "Presets/Audio Effects/Max Audio Effect/Alice Gate.amxd"
            ),
            (
                ABLETON_USER_LIBRARY,
                "Presets/Audio Effects/Reverb/Big Hall.adv"
            ),
            (
                ABLETON_USER_LIBRARY,
                "Presets/Instruments/Drum Rack/Alice Kit.adg"
            ),
            (ABLETON_USER_LIBRARY, "Templates/Default.als"),
            (ABLETON_PREFS, "Options.txt"),
            (ABLETON_PREFS, "Library.cfg"),
        ]
    );
    let labels: Vec<&str> = items.iter().map(|i| describe(&i.path)).collect();
    assert_eq!(
        labels,
        [
            "Collections",
            "Clip",
            "Default preset",
            "Max for Live device",
            "Preset",
            "Rack",
            "Template",
            "Setting",
            "Collection names"
        ]
    );

    for (item, product) in items.iter().zip(&products) {
        assert!(is_ableton_item(item));
        assert_eq!(item.product_id.as_ref(), Some(&product.canonical_id));
        assert_eq!(product.vendor, VENDOR);
        assert!(item.content_hash.is_some() && item.platform_portable);
    }
    let rack = &items[5];
    assert_eq!(
        rack.requires,
        [
            ProductId::from("Acme Audio/Acme Synth"),
            ProductId::from("unknown/Acme Comp x64"),
            ProductId::from("unknown/Acme Verb"),
        ]
    );
    assert!(
        items
            .iter()
            .filter(|i| i != &rack)
            .all(|i| i.requires.is_empty())
    );
    let kinds: Vec<ItemKind> = items.iter().map(|i| i.kind).collect();
    assert_eq!(kinds[0], ItemKind::Config);
    assert_eq!(kinds[6], ItemKind::DawAsset);
    assert_eq!(kinds[7], ItemKind::Config);
    assert_eq!(kinds[8], ItemKind::Config);
    assert_eq!(products[8].name, "Collection names");
}

#[test]
fn same_file_on_mac_and_pc_is_the_same_product_and_hash() {
    let (_, mac) = settings_items(
        None,
        Some(&fixture("prefs-mac/Live 12.0.1")),
        Os::Macos,
        by_name,
    );
    let (_, win) = settings_items(
        None,
        Some(&fixture("prefs-win/Live 12.1.5/Preferences")),
        Os::Windows,
        by_name,
    );
    assert_eq!(mac[0].product_id, win[0].product_id);
    assert_eq!(mac[0].content_hash, win[0].content_hash);
}

#[test]
fn missing_folders_give_nothing() {
    let (products, items) = settings_items(Some(Path::new("/nope")), None, Os::Macos, by_name);
    assert!(products.is_empty() && items.is_empty());
}

#[test]
fn reads_plugins_from_a_rack_and_skips_saved_state() {
    let bytes = std::fs::read(fixture(
        "User Library/Presets/Instruments/Drum Rack/Alice Kit.adg",
    ))
    .unwrap();
    let refs = parse_live_file(&bytes);
    let summary: Vec<(PluginFormat, &str, &str)> = refs
        .iter()
        .map(|r| (r.format, r.name.as_str(), r.key.as_str()))
        .collect();
    assert_eq!(
        summary,
        [
            (PluginFormat::Vst2, "Acme Comp x64", "1094807365"),
            (
                PluginFormat::Vst3,
                "Acme Verb",
                "56535343FFFFFFFF0000000000000010"
            ),
            (PluginFormat::Vst3, "Acme Verb", "Acme Verb"),
            (PluginFormat::Au, "Acme Synth", "Acme Audio: Acme Synth"),
        ]
    );
}

#[test]
fn collection_names_travel_without_paths() {
    let mac = std::fs::read_to_string(fixture("prefs-mac/Live 12.1.5/Library.cfg")).unwrap();
    let pc =
        std::fs::read_to_string(fixture("prefs-win/Live 12.1.5/Preferences/Library.cfg")).unwrap();
    assert_eq!(
        collection_names(&mac)[..2],
        [(1, "Faves".to_owned()), (2, "Synths & Keys".to_owned())]
    );

    // What the PC shares: names only, no paths.
    let shared = shared_view(ABLETON_PREFS, LIBRARY_CFG, pc.clone().into_bytes());
    let text = String::from_utf8(shared.clone()).unwrap();
    assert!(text.starts_with("1\tFavorites\n2\tInst\n3\tRacks\n"));
    assert!(!text.contains("D:"));

    // The Mac takes the PC's names and keeps its own paths and Places.
    let merged = merge_incoming(ABLETON_PREFS, LIBRARY_CFG, Some(mac.as_bytes()), shared).unwrap();
    let merged = String::from_utf8(merged).unwrap();
    assert_eq!(collection_names(&merged), collection_names(&pc));
    assert_eq!(
        user_library_from_cfg(&merged),
        Some(PathBuf::from("/Users/alice/Music/Ableton/User Library"))
    );
    assert!(merged.contains(r#"Path="/Users/alice/Samples/Breaks""#));
    assert!(!merged.contains("D:"));
    // Nothing else moved: only DisplayName values differ, line by line.
    for (a, b) in mac.lines().zip(merged.lines()) {
        if a != b {
            assert!(a.contains("Uri=\"color:colors="), "{a}");
        }
    }
    assert_eq!(mac.lines().count(), merged.lines().count());

    // Escaping round-trips.
    let back = merge_incoming(
        ABLETON_PREFS,
        LIBRARY_CFG,
        Some(merged.as_bytes()),
        shared_view(ABLETON_PREFS, LIBRARY_CFG, mac.clone().into_bytes()),
    )
    .unwrap();
    assert_eq!(String::from_utf8(back).unwrap(), mac);
}

#[test]
fn collection_names_need_a_library_cfg_with_collections() {
    let names = b"1\tFaves\n".to_vec();
    assert!(merge_incoming(ABLETON_PREFS, LIBRARY_CFG, None, names.clone()).is_err());
    assert!(
        merge_incoming(
            ABLETON_PREFS,
            LIBRARY_CFG,
            Some(b"<Ableton/>"),
            names.clone()
        )
        .is_err()
    );
    let cfg = std::fs::read(fixture("prefs-mac/Live 12.1.5/Library.cfg")).unwrap();
    assert!(merge_incoming(ABLETON_PREFS, LIBRARY_CFG, Some(&cfg), b"junk".to_vec()).is_err());
    // Other files pass through untouched.
    assert_eq!(
        merge_incoming(ABLETON_PREFS, "Options.txt", None, names.clone()).unwrap(),
        names
    );
}
