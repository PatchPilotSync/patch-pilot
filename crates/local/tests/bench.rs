//! Open-the-app budget: reading a big rig with a long history must stay fast,
//! or the window sits blank while it works (2026-10-26).
//!
//! Builds a synthetic rig of 1,500 plug-ins in two formats plus 2,000
//! presets with 30 saved scans, then times what the app does on opening:
//! the snapshot list, the latest manifest and a comparison with the Primary
//! Rig, plus what History and the Installer Vault read when they open
//! (2026-10-27: both took seconds on a real rig). Ignored by default; CI
//! runs it:
//! `cargo test -p patchpilot-local --test bench -- --ignored --nocapture`

use std::time::{Duration, Instant};

use patchpilot_core::{ItemKind, Manifest, TargetProfile};
use patchpilot_store::Store;

const PRODUCTS: usize = 1_500;
const PRESETS: usize = 2_000;
const SCANS: usize = 30;
const BUDGET: Duration = Duration::from_secs(5);

/// A rig where scan `n` has `n` extra presets, so every scan differs a little.
fn rig(id: &str, os: &str, extra: usize) -> Manifest {
    let mut products = Vec::new();
    let mut items = Vec::new();
    for p in 0..PRODUCTS {
        products.push(format!(
            r#"{{"canonical_id":"vendor{v}-product{p}","vendor":"Vendor {v}","name":"Product {p}"}}"#,
            v = p % 40
        ));
        for (format, root, ext) in [
            ("vst3", "VST3_SYSTEM", "vst3"),
            ("clap", "CLAP_SYSTEM", "clap"),
        ] {
            items.push(format!(
                r#"{{"kind":"plugin","product_id":"vendor{v}-product{p}","format":"{format}","platform":"{os}","path":{{"logical":{{"root":"{root}","rel":"Vendor {v}/Product {p}.{ext}"}}}},"confidence":"high","version":"1.{p}.0"}}"#,
                v = p % 40
            ));
        }
    }
    for n in 0..PRESETS + extra {
        let p = n % PRODUCTS;
        products.push(format!(
            r#"{{"canonical_id":"preset:{n}","vendor":"Vendor {v}","name":"Preset {n}"}}"#,
            v = p % 40
        ));
        items.push(format!(
            r#"{{"kind":"config","product_id":"preset:{n}","platform":"{os}","path":{{"logical":{{"root":"VST3_PRESETS","rel":"Vendor {v}/Product {p}/Preset {n}.vstpreset"}}}},"content_hash":"blake3:{h:0>64x}","size":2048,"mtime":1790000000,"platform_portable":true,"confidence":"high"}}"#,
            v = p % 40,
            h = n
        ));
    }
    let json = format!(
        r#"{{"schema_version":1,"machine":{{"id":"{id}","name":"{id}","os":"{os}","arch":"x86_64","path_map":{{"os":"{os}","roots":{{}}}}}},"generated_at":{extra},"products":[{}],"items":[{}]}}"#,
        products.join(","),
        items.join(",")
    );
    Manifest::from_json(&json).expect("synthetic manifest")
}

fn time<T>(label: &str, f: impl FnOnce() -> T) -> (T, Duration) {
    let start = Instant::now();
    let out = f();
    let took = start.elapsed();
    println!("{label}: {took:?}");
    (out, took)
}

#[test]
#[ignore = "budget test; run with --ignored"]
fn opening_a_big_rig_with_a_long_history() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(&dir.path().join("pp.db")).unwrap();
    let golden = rig("studio", "windows", 0);
    patchpilot_local::save_scan(&mut store, &golden).unwrap();
    store.save_golden(&golden, Some("studio"), None, 1).unwrap();
    for n in 1..SCANS {
        patchpilot_local::save_scan(&mut store, &rig("laptop", "macos", n)).unwrap();
    }

    let (history, list) = time("snapshot list", || store.snapshots("laptop").unwrap());
    assert_eq!(history.len(), SCANS - 1);
    let (_, latest) = time("latest scan time", || {
        store.latest_snapshot("laptop").unwrap().unwrap()
    });
    let (machine, read) = time("latest manifest", || {
        store.latest_manifest("laptop").unwrap().unwrap()
    });
    let (c, diff) = time("comparison", || {
        patchpilot_parity::compare(&golden, &TargetProfile::default(), &machine)
    });
    assert!(!c.rows.is_empty());
    let (_, save) = time("rescan", || {
        patchpilot_local::save_scan(&mut store, &rig("laptop", "macos", SCANS)).unwrap()
    });

    // The Primary Rig refreshes its files' fingerprints from the machine it
    // came from: a join that once walked every pair of items.
    let (g, primary) = time("Primary Rig", || store.golden(None).unwrap().unwrap());
    assert_eq!(g.manifest.items.len(), golden.items.len());
    // History: the plug-ins of every scan, each compared with the one before.
    let (changes, history) = time("History", || {
        let scans = store.snapshot_times("laptop").unwrap();
        let plugins: Vec<Manifest> = scans
            .iter()
            .map(|s| {
                store
                    .snapshot_manifest_of(s.id, &[ItemKind::Plugin])
                    .unwrap()
            })
            .collect();
        plugins
            .windows(2)
            .map(|w| patchpilot_parity::plugin_changes(&w[1], &w[0]).len())
            .sum::<usize>()
    });
    assert_eq!(changes, 0, "only presets differ between scans");
    // The Installer Vault: everything installable on any machine.
    let (products, vault) = time("Vault products", || {
        let (id, _) = store.latest_snapshot("laptop").unwrap().unwrap();
        let kinds = [ItemKind::Plugin, ItemKind::VendorApp, ItemKind::LicenseTool];
        let laptop = store.snapshot_manifest_of(id, &kinds).unwrap();
        patchpilot_local::vault::installable_products([&g.manifest, &laptop])
    });
    assert_eq!(products.len(), PRODUCTS);

    let total = list + latest + read + diff + save;
    println!("total: {total:?} (budget {BUDGET:?})");
    assert!(total < BUDGET, "took {total:?}, budget {BUDGET:?}");
    // Each tab on its own (debug build; a release build is several times
    // faster). History had taken ~6 s on a real rig, the Primary Rig ~2 s.
    for (tab, took, budget) in [
        ("Primary Rig", primary, Duration::from_secs(1)),
        ("History", history, Duration::from_secs(4)),
        ("Vault", vault, Duration::from_secs(1)),
    ] {
        assert!(took < budget, "{tab} took {took:?}, budget {budget:?}");
    }
}
