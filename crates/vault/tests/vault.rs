//! Adding installers to a vault folder, finding the right one, checking it.

use std::fs;

use patchpilot_core::{Arch, Os, ProductId};
use patchpilot_vault::{Added, INDEX_FILE, NewEntry, Vault, installers_in};

fn meta(version: &str, os: Option<Os>) -> NewEntry {
    NewEntry {
        vendor: "FabFilter".into(),
        product: "Pro-Q 3".into(),
        product_id: Some(ProductId("fabfilter/pro-q-3".into())),
        version: Some(version.into()),
        os,
        arch: Some(Arch::X86_64),
    }
}

#[test]
fn adds_copies_into_a_readable_layout_and_leaves_the_original() {
    let dir = tempfile::tempdir().unwrap();
    let downloads = dir.path().join("Downloads");
    fs::create_dir_all(&downloads).unwrap();
    let src = downloads.join("FabFilter Pro-Q 3 v3.21 Installer.exe");
    fs::write(&src, b"installer bytes").unwrap();

    let vault = Vault::new(dir.path().join("Vault"));
    let added = vault
        .add(
            &src,
            meta("3.21", Some(Os::Windows)),
            Some("FabFilter B.V.".into()),
            100,
        )
        .unwrap();
    let Added::New(entry) = &added else {
        panic!("expected a new entry, got {added:?}");
    };
    assert_eq!(
        entry.file,
        "FabFilter/Pro-Q 3/3.21/windows-x64/FabFilter Pro-Q 3 v3.21 Installer.exe"
    );
    assert!(src.exists(), "the download is left alone");
    assert_eq!(fs::read(vault.path_of(entry)).unwrap(), b"installer bytes");
    assert!(vault.verify(entry).unwrap());
    assert_eq!(vault.index().unwrap().entries, std::slice::from_ref(entry));

    // Nothing temporary is left behind.
    let names: Vec<_> = fs::read_dir(&vault.root)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(names.len(), 2, "{names:?}");
    assert!(names.contains(&INDEX_FILE.to_owned()));

    // The same bytes again, under another name, aren't added twice.
    let again = downloads.join("copy.exe");
    fs::write(&again, b"installer bytes").unwrap();
    let second = vault
        .add(&again, meta("3.21", Some(Os::Windows)), None, 200)
        .unwrap();
    assert_eq!(second, Added::AlreadyThere(entry.clone()));
    assert_eq!(vault.index().unwrap().entries.len(), 1);
}

#[test]
fn a_different_file_with_the_same_name_never_becomes_a_numbered_copy() {
    let dir = tempfile::tempdir().unwrap();
    let vault = Vault::new(dir.path().join("Vault"));
    let a = dir.path().join("a");
    let b = dir.path().join("b");
    fs::create_dir_all(&a).unwrap();
    fs::create_dir_all(&b).unwrap();
    fs::write(a.join("Setup.exe"), b"one").unwrap();
    fs::write(b.join("Setup.exe"), b"two").unwrap();
    let one = vault
        .add(
            &a.join("Setup.exe"),
            meta("1.0", Some(Os::Windows)),
            None,
            1,
        )
        .unwrap();
    let two = vault
        .add(
            &b.join("Setup.exe"),
            meta("1.0", Some(Os::Windows)),
            None,
            2,
        )
        .unwrap();
    assert_ne!(one.entry().file, two.entry().file);
    assert!(
        two.entry()
            .file
            .ends_with(&format!("{}/Setup.exe", &two.entry().hash[..8]))
    );
    assert!(!two.entry().file.contains("(1)"));
    assert!(vault.verify(one.entry()).unwrap() && vault.verify(two.entry()).unwrap());
}

#[test]
fn picks_the_newest_installer_for_this_os_and_notices_changes() {
    let dir = tempfile::tempdir().unwrap();
    let vault = Vault::new(dir.path().join("Vault"));
    let add = |name: &str, body: &[u8], version: &str, os: Option<Os>| {
        let p = dir.path().join(name);
        fs::write(&p, body).unwrap();
        vault
            .add(&p, meta(version, os), None, 0)
            .unwrap()
            .entry()
            .clone()
    };
    add("old.exe", b"1", "3.10", Some(Os::Windows));
    let new = add("new.exe", b"2", "3.21", Some(Os::Windows));
    let mac = add("mac.pkg", b"3", "3.30", Some(Os::Macos));
    let any = add("both.zip", b"4", "3.40", None);

    let id = ProductId("fabfilter/pro-q-3".into());
    // A Windows installer beats a newer zip that doesn't say its OS.
    assert_eq!(vault.best_for(&id, Os::Windows).unwrap(), Some(new.clone()));
    assert_eq!(vault.best_for(&id, Os::Macos).unwrap(), Some(mac));
    assert_eq!(
        vault
            .best_for(&ProductId("other/x".into()), Os::Macos)
            .unwrap(),
        None
    );
    let _ = any;

    fs::write(vault.path_of(&new), b"tampered").unwrap();
    assert!(!vault.verify(&new).unwrap());
}

#[test]
fn lists_finished_installer_downloads() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["a.dmg", "b.exe.crdownload", "song.wav", "c.ZIP"] {
        fs::write(dir.path().join(name), b"x").unwrap();
    }
    fs::create_dir_all(dir.path().join("sub.pkg")).unwrap();
    let mut names: Vec<_> = installers_in(dir.path())
        .into_iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, ["a.dmg", "c.ZIP"]);
}

#[test]
fn refuses_an_index_from_a_newer_app() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(INDEX_FILE),
        r#"{"version": 99, "entries": []}"#,
    )
    .unwrap();
    assert!(Vault::new(dir.path()).index().is_err());
}

#[test]
fn removing_undoes_an_add_and_tidies_empty_folders() {
    let dir = tempfile::tempdir().unwrap();
    let vault = Vault::new(dir.path().join("Vault"));
    let src = dir.path().join("Pro-Q.exe");
    fs::write(&src, b"q").unwrap();
    let keep = dir.path().join("Other.exe");
    fs::write(&keep, b"other").unwrap();
    let kept = vault
        .add(&keep, meta("1.0", Some(Os::Windows)), None, 0)
        .unwrap();
    let added = vault
        .add(&src, meta("3.21", Some(Os::Windows)), None, 0)
        .unwrap();

    let removed = vault.remove(&added.entry().id).unwrap();
    assert_eq!(&removed, added.entry());
    assert!(src.exists(), "the download is untouched");
    assert!(!vault.path_of(added.entry()).exists());
    assert!(
        !vault.root.join("FabFilter/Pro-Q 3/3.21").exists(),
        "empty folders go"
    );
    assert!(
        vault.root.join("FabFilter/Pro-Q 3/1.0").exists(),
        "others stay"
    );
    assert_eq!(
        vault.index().unwrap().entries,
        std::slice::from_ref(kept.entry())
    );
    assert!(vault.remove(&added.entry().id).is_err());
}

#[test]
fn adds_at_the_same_time_all_land() {
    let dir = tempfile::tempdir().unwrap();
    let vault = Vault::new(dir.path().join("Vault"));
    let files: Vec<_> = (0..4)
        .map(|i| {
            let p = dir.path().join(format!("Setup{i}.exe"));
            fs::write(&p, vec![i as u8; 200_000]).unwrap();
            p
        })
        .collect();
    std::thread::scope(|s| {
        for f in &files {
            let vault = &vault;
            s.spawn(move || {
                vault
                    .add(f, meta("1.0", Some(Os::Windows)), None, 0)
                    .unwrap()
            });
        }
    });
    let entries = vault.index().unwrap().entries;
    assert_eq!(entries.len(), 4);
    for e in &entries {
        assert!(vault.verify(e).unwrap());
    }
}

#[test]
fn an_index_entry_can_never_point_outside_the_vault() {
    let dir = tempfile::tempdir().unwrap();
    let victim = dir.path().join("victim.txt");
    fs::write(&victim, b"keep me").unwrap();
    let src = dir.path().join("Pro-Q 3 Installer.exe");
    fs::write(&src, b"installer bytes").unwrap();
    let vault = Vault::new(dir.path().join("Vault"));
    let added = vault.add(&src, meta("3.21", None), None, 0).unwrap();

    // Someone edits vault.json (or a restore brings an edited one back).
    let index_path = vault.root.join(INDEX_FILE);
    let mut index: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&index_path).unwrap()).unwrap();
    for bad in ["../victim.txt", "a/../../victim.txt", "C:\\victim.txt"] {
        index["entries"][0]["file"] = bad.into();
        fs::write(&index_path, serde_json::to_string(&index).unwrap()).unwrap();
        let entry = vault.entry(&added.entry().id).unwrap();
        assert!(vault.path_of(&entry).starts_with(&vault.root), "{bad}");
        assert!(vault.verify(&entry).is_err() || !vault.verify(&entry).unwrap());
        let _ = vault.remove(&entry.id);
        assert_eq!(fs::read(&victim).unwrap(), b"keep me", "{bad}");
        // Put the entry back for the next try.
        vault.add(&src, meta("3.21", None), None, 0).unwrap();
        index = serde_json::from_str(&fs::read_to_string(&index_path).unwrap()).unwrap();
    }
}
