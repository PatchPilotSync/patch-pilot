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

/// Merge shared `Library.cfg` text into a Mac's file, with no cloud folders.
fn merge(current: Option<&[u8]>, incoming: Vec<u8>) -> Result<Vec<u8>, String> {
    merge_incoming(
        ABLETON_PREFS,
        LIBRARY_CFG,
        current,
        incoming,
        &[],
        Os::Macos,
    )
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
    let (products, items) = settings_items(Some(&lib), Some(&prefs), &[], Os::Macos, by_name);

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
            "Collection names and Places"
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
    assert_eq!(products[8].name, "Collection names and Places");
}

#[test]
fn same_file_on_mac_and_pc_is_the_same_product_and_hash() {
    let (_, mac) = settings_items(
        None,
        Some(&fixture("prefs-mac/Live 12.0.1")),
        &[],
        Os::Macos,
        by_name,
    );
    let (_, win) = settings_items(
        None,
        Some(&fixture("prefs-win/Live 12.1.5/Preferences")),
        &[],
        Os::Windows,
        by_name,
    );
    assert_eq!(mac[0].product_id, win[0].product_id);
    assert_eq!(mac[0].content_hash, win[0].content_hash);
}

#[test]
fn missing_folders_give_nothing() {
    let (products, items) = settings_items(Some(Path::new("/nope")), None, &[], Os::Macos, by_name);
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
    let shared = shared_view(ABLETON_PREFS, LIBRARY_CFG, pc.clone().into_bytes(), &[]);
    let text = String::from_utf8(shared.clone()).unwrap();
    assert!(text.starts_with("1\tFavorites\n2\tInst\n3\tRacks\n"));
    assert!(!text.contains("D:"));

    // The Mac takes the PC's names and keeps its own paths and Places.
    let merged = merge(Some(mac.as_bytes()), shared).unwrap();
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
    let back = merge(
        Some(merged.as_bytes()),
        shared_view(ABLETON_PREFS, LIBRARY_CFG, mac.clone().into_bytes(), &[]),
    )
    .unwrap();
    assert_eq!(String::from_utf8(back).unwrap(), mac);
}

#[test]
fn collection_names_need_a_library_cfg_with_collections() {
    let names = b"1\tFaves\n".to_vec();
    assert!(merge(None, names.clone()).is_err());
    assert!(merge(Some(b"<Ableton/>"), names.clone()).is_err());
    let cfg = std::fs::read(fixture("prefs-mac/Live 12.1.5/Library.cfg")).unwrap();
    assert!(merge(Some(&cfg), b"junk".to_vec()).is_err());
    // Other files pass through untouched.
    assert_eq!(
        merge_incoming(
            ABLETON_PREFS,
            "Options.txt",
            None,
            names.clone(),
            &[],
            Os::Macos
        )
        .unwrap(),
        names
    );
}

/// A `Library.cfg` with these Collection names' first colour and Places.
fn cfg_with(user_library: &str, places: &[(&str, &str)]) -> String {
    let list: String = places
        .iter()
        .enumerate()
        .map(|(i, (path, name))| {
            format!(
                "\n\t\t\t<UserFolderInfo Id=\"{}\" Path=\"{path}\" DisplayName=\"{name}\" IconName=\"\" />",
                100 + i
            )
        })
        .collect();
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Ableton>\n\t<ContentLibrary>\n\t\t<UserLibrary>\n\t\t\t<LibraryProject Id=\"1\">\n\t\t\t\t<ProjectPath Value=\"{user_library}\" />\n\t\t\t</LibraryProject>\n\t\t</UserLibrary>\n\t\t<UserFolderInfoList>{list}\n\t\t</UserFolderInfoList>\n\t\t<LabelInfos>\n\t\t\t<LabelInfo Id=\"250572\" Uri=\"color:colors=1\" DisplayName=\"Faves\" IconName=\"\" />\n\t\t</LabelInfos>\n\t</ContentLibrary>\n</Ableton>\n"
    )
}

#[test]
fn places_in_dropbox_and_the_user_library_travel_and_others_stay() {
    let tmp = tempfile::tempdir().unwrap();
    let mac_dropbox = tmp.path().join("Dropbox");
    std::fs::create_dir_all(mac_dropbox.join("Samples/JUNGLE BREAKS")).unwrap();
    // Spelled with `/`, as the Mac writes it, also when the test runs on Windows.
    let mac_dropbox = mac_dropbox.to_string_lossy().replace('\\', "/");
    let pc_clouds = [PlaceRoot {
        name: "Dropbox".into(),
        path: r"D:\Dropbox".into(),
    }];
    let mac_clouds = [PlaceRoot {
        name: "Dropbox".into(),
        path: mac_dropbox.clone(),
    }];
    let pc = cfg_with(
        "D:/Music/!User Library/",
        &[
            (r"D:\Dropbox\Samples\JUNGLE BREAKS", "JUNGLE BREAKS"),
            (r"d:\dropbox\Acapellas", "Acapellas &amp; Vox"),
            (r"D:\Samples\Breaks", "Breaks"),
            (r"D:\Music\!User Library\Racks", "Ableton Racks"),
        ],
    );
    let old = format!("{mac_dropbox}/Old");
    let mac = cfg_with(
        "/Users/alice/Music/Ableton/User Library",
        &[("/Users/alice/Samples/Breaks", "Breaks"), (&old, "Old")],
    );

    // The PC shares its Places relative to Dropbox and the User Library;
    // the one on another drive stays its own.
    let shared = shared_view(
        ABLETON_PREFS,
        LIBRARY_CFG,
        pc.clone().into_bytes(),
        &pc_clouds,
    );
    let text = String::from_utf8(shared.clone()).unwrap();
    assert_eq!(
        text,
        "1\tFaves\nhidden\nplaces\n{Dropbox}/Samples/JUNGLE BREAKS\tJUNGLE BREAKS\n\
         {Dropbox}/Acapellas\tAcapellas & Vox\n{User Library}/Racks\tAbleton Racks\n"
    );

    // The Mac gets them pointed at its own folders, in the PC's order, keeps
    // its Place on another drive where it was and drops the Dropbox one the
    // PC removed.
    let merged = merge_incoming(
        ABLETON_PREFS,
        LIBRARY_CFG,
        Some(mac.as_bytes()),
        shared.clone(),
        &mac_clouds,
        Os::Macos,
    )
    .unwrap();
    let merged = String::from_utf8(merged).unwrap();
    let got: Vec<(String, String)> = places(&merged)
        .into_iter()
        .map(|p| (p.path, p.name))
        .collect();
    assert_eq!(
        got,
        [
            (
                "/Users/alice/Samples/Breaks".to_owned(),
                "Breaks".to_owned()
            ),
            (
                format!("{mac_dropbox}/Samples/JUNGLE BREAKS"),
                "JUNGLE BREAKS".to_owned()
            ),
            (
                format!("{mac_dropbox}/Acapellas"),
                "Acapellas & Vox".to_owned()
            ),
            (
                "/Users/alice/Music/Ableton/User Library/Racks".to_owned(),
                "Ableton Racks".to_owned()
            ),
        ]
    );
    // New Places get unused ids; the Mac's kept one keeps its own.
    assert!(merged.contains(r#"<UserFolderInfo Id="250573" Path=""#));
    assert!(merged.contains(r#"<UserFolderInfo Id="100" Path="/Users/alice/Samples/Breaks""#));
    assert!(merged.contains("\n\t\t\t<UserFolderInfo Id=\"250573\""));
    assert!(merged.contains("\n\t\t</UserFolderInfoList>"));
    // Both machines now share the same, so nothing goes back and forth.
    assert_eq!(
        shared_view(
            ABLETON_PREFS,
            LIBRARY_CFG,
            merged.clone().into_bytes(),
            &mac_clouds
        ),
        shared
    );
    // Merging again changes nothing.
    let again = merge_incoming(
        ABLETON_PREFS,
        LIBRARY_CFG,
        Some(merged.as_bytes()),
        shared.clone(),
        &mac_clouds,
        Os::Macos,
    )
    .unwrap();
    assert_eq!(String::from_utf8(again).unwrap(), merged);

    // The Mac lists what isn't there: Acapellas isn't in its Dropbox yet,
    // and its User Library has no Racks folder.
    assert_eq!(
        places_not_here(&text, &merged, &mac_clouds),
        ["Acapellas & Vox", "Ableton Racks"]
    );
    // Without Dropbox here, its Places can't come: only the rest does.
    let no_dropbox = merge_incoming(
        ABLETON_PREFS,
        LIBRARY_CFG,
        Some(mac.as_bytes()),
        shared.clone(),
        &[],
        Os::Macos,
    )
    .unwrap();
    let names: Vec<String> = places(&String::from_utf8(no_dropbox).unwrap())
        .into_iter()
        .map(|p| p.name)
        .collect();
    assert_eq!(names, ["Breaks", "Old", "Ableton Racks"]);

    // Back the other way: the PC writes its own `\` paths.
    let back = merge_incoming(
        ABLETON_PREFS,
        LIBRARY_CFG,
        Some(pc.as_bytes()),
        shared_view(ABLETON_PREFS, LIBRARY_CFG, merged.into_bytes(), &mac_clouds),
        &pc_clouds,
        Os::Windows,
    )
    .unwrap();
    assert_eq!(String::from_utf8(back).unwrap(), pc);
}

#[test]
fn names_from_before_places_travelled_leave_places_alone() {
    let mac = cfg_with(
        "/Users/alice/Music/Ableton/User Library",
        &[("/x/Old", "Old")],
    );
    assert!(is_names_only(b"1\tKicks\n"));
    assert!(!is_names_only(b"1\tKicks\nplaces\n"));
    let merged = merge(Some(mac.as_bytes()), b"1\tKicks\n".to_vec()).unwrap();
    let merged = String::from_utf8(merged).unwrap();
    assert_eq!(collection_names(&merged), [(1, "Kicks".to_owned())]);
    assert_eq!(places(&merged).len(), 1);
    // Every Place leaving makes the list empty, as Live writes it.
    let none = merge(Some(mac.as_bytes()), b"1\tKicks\nplaces\n".to_vec()).unwrap();
    assert_eq!(
        places(&String::from_utf8(none).unwrap()).len(),
        1,
        "a Place outside shared folders stays"
    );
}

#[test]
fn collections_hidden_on_one_machine_are_hidden_on_the_other() {
    let mac = std::fs::read_to_string(fixture("prefs-mac/Live 12.1.5/Library.cfg")).unwrap();
    // On the PC, Collection 3 and the Clips category are hidden with Edit.
    let pc = mac
        .replace(
            r#"Uri="color:colors=3" DisplayName="" IconName="" Settings="" IsEnabled="true""#,
            r#"Uri="color:colors=3" DisplayName="" IconName="" Settings="" IsEnabled="false""#,
        )
        .replace(
            r#"Uri="view:X-Clips" DisplayName="" IconName="" Settings="" IsEnabled="true""#,
            r#"Uri="view:X-Clips" DisplayName="" IconName="" Settings="" IsEnabled="false""#,
        );
    assert_ne!(pc, mac);
    let shared = shared_view(ABLETON_PREFS, LIBRARY_CFG, pc.clone().into_bytes(), &[]);
    let text = String::from_utf8(shared.clone()).unwrap();
    assert!(
        text.contains("\nhidden\ncolor:colors=3\nview:X-Clips\nplaces\n"),
        "{text}"
    );

    let merged = String::from_utf8(merge(Some(mac.as_bytes()), shared.clone()).unwrap()).unwrap();
    assert_eq!(merged, pc);
    // And shown again the other way.
    let back = merge(
        Some(merged.as_bytes()),
        shared_view(ABLETON_PREFS, LIBRARY_CFG, mac.clone().into_bytes(), &[]),
    )
    .unwrap();
    assert_eq!(String::from_utf8(back).unwrap(), mac);
}

#[test]
fn a_real_windows_library_cfg_shares_names_hidden_collections_and_dropbox_places() {
    // Live 12.4.7 on Windows, as a user had it: Collections 6 and 7 removed
    // with the sidebar's Edit, and Places in Dropbox and on another drive.
    let pc = std::fs::read_to_string(fixture("Library-12.4.7-windows.cfg")).unwrap();
    let dropbox = [PlaceRoot {
        name: "Dropbox".into(),
        path: r"D:\Dropbox".into(),
    }];
    let text = library_cfg_text(&pc, &dropbox);
    assert_eq!(
        text,
        "1\tFavorites\n2\tInst\n3\tChains\n4\tDynamics and Color\n5\tTime\n6\tNot Purple\n7\tMisc\n\
         hidden\ncolor:colors=6\ncolor:colors=7\n\
         places\n\
         {Dropbox}/Audio/Samples/Acapellas\tAcapellas\n\
         {Dropbox}/Audio/Project Files/Ableton Sessions/!User Library/Ableton Racks\tAbleton Racks\n\
         {Dropbox}/Audio/Project Files/Ableton Sessions/Ableton MaxLive\tAbleton MaxLive\n\
         {Dropbox}/Audio/Samples/JUNGLE BREAKS\tJUNGLE BREAKS\n"
    );

    // A Mac with Dropbox in its File Provider folder gets all of it.
    let mac = std::fs::read_to_string(fixture("prefs-mac/Live 12.1.5/Library.cfg")).unwrap();
    let mac_dropbox = [PlaceRoot {
        name: "Dropbox".into(),
        path: "/Users/alice/Library/CloudStorage/Dropbox".into(),
    }];
    let merged = merge_incoming(
        ABLETON_PREFS,
        LIBRARY_CFG,
        Some(mac.as_bytes()),
        text.clone().into_bytes(),
        &mac_dropbox,
        Os::Macos,
    )
    .unwrap();
    let merged = String::from_utf8(merged).unwrap();
    assert_eq!(collection_names(&merged), collection_names(&pc));
    assert!(merged.contains(
        r#"Uri="color:colors=6" DisplayName="Not Purple" IconName="" Settings="" IsEnabled="false""#
    ));
    assert!(merged.contains(
        r#"Uri="color:colors=5" DisplayName="Time" IconName="" Settings="" IsEnabled="true""#
    ));
    assert!(merged.contains(
        r#"Path="/Users/alice/Library/CloudStorage/Dropbox/Audio/Samples/JUNGLE BREAKS" DisplayName="JUNGLE BREAKS""#
    ));
    assert_eq!(library_cfg_text(&merged, &mac_dropbox), text);
}
