//! "Propose to Golden" and drift alerts on this machine (M9): add a
//! suggestion from any machine to the Golden Rig, ignore one, and work out
//! which alerts are new. The file of what the user was last told lives in
//! app-data, never in user folders.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use patchpilot_core::GoldenVersionInfo;
use patchpilot_parity::{Alert, Observed, Seen, compare, propose_to_golden, suggestion_key};
use patchpilot_store::Store;

const SEEN_FILE: &str = "alerts-seen.json";
const OFF_FILE: &str = "drift-alerts-off";

/// Add `products` (IDs) from `machine_id`'s latest scan to a new Golden
/// version, noting where they came from.
pub fn add_to_golden(
    store: &mut Store,
    machine_id: &str,
    products: &[String],
    now: i64,
) -> Result<GoldenVersionInfo, String> {
    let golden = store
        .golden(None)
        .map_err(err)?
        .ok_or("there's no Golden Rig yet")?;
    let scan = store
        .latest_manifest(machine_id)
        .map_err(err)?
        .ok_or("no saved scan of that machine")?;
    let next = propose_to_golden(&golden.manifest, &scan, products).map_err(|e| e.to_string())?;
    let what = match products {
        [one] => scan
            .products
            .iter()
            .find(|p| p.canonical_id.0 == *one)
            .map_or_else(|| one.clone(), |p| p.name.clone()),
        many => format!("{} products", many.len()),
    };
    let note = format!("Added {what} from {}", scan.machine.name);
    store
        .save_golden(&next, Some(machine_id), Some(&note), now)
        .map_err(err)
}

/// [`suggestion_key`]s the user ignored, on this or any linked machine.
pub fn ignored_keys(store: &Store) -> Result<BTreeSet<String>, String> {
    Ok(store
        .ignored_suggestions()
        .map_err(err)?
        .iter()
        .map(|i| suggestion_key(&i.product_id, i.version.as_deref()))
        .collect())
}

/// Every machine against the latest Golden through its profile. Empty
/// without a Golden Rig.
pub fn observe(store: &Store, this: Option<&str>) -> Result<Vec<Observed>, String> {
    let Some(golden) = store.golden(None).map_err(err)? else {
        return Ok(Vec::new());
    };
    let ignored = ignored_keys(store)?;
    let mut out = Vec::new();
    for info in store.machines().map_err(err)? {
        let Some(scan) = store.latest_manifest(&info.id).map_err(err)? else {
            continue;
        };
        let profile = store
            .machine_profile(&info.id)
            .map_err(err)?
            .unwrap_or_default();
        let c = compare(&golden.manifest, &profile, &scan);
        out.push(Observed::new(
            &info.id,
            &info.name,
            Some(info.id.as_str()) == this,
            &c,
            &ignored,
        ));
    }
    Ok(out)
}

/// Alerts the user hasn't had yet, and remember them as told. While alerts
/// are off this still records the current state, so turning them on later
/// doesn't replay old news.
pub fn check_alerts(
    store: &Store,
    app_dir: &Path,
    this: Option<&str>,
) -> Result<Vec<Alert>, String> {
    let (alerts, seen) = patchpilot_parity::alerts(&load_seen(app_dir), &observe(store, this)?);
    save_seen(app_dir, &seen)?;
    Ok(if alerts_enabled(app_dir) {
        alerts
    } else {
        Vec::new()
    })
}

/// Record the current state without alerting, after a change the user made
/// here (a new Golden version, an ignored suggestion).
pub fn rebaseline(store: &Store, app_dir: &Path, this: Option<&str>) -> Result<(), String> {
    let (_, seen) = patchpilot_parity::alerts(&load_seen(app_dir), &observe(store, this)?);
    save_seen(app_dir, &seen)
}

/// Drift alerts are on unless the user turned them off.
pub fn alerts_enabled(app_dir: &Path) -> bool {
    !app_dir.join(OFF_FILE).exists()
}

pub fn set_alerts_enabled(app_dir: &Path, on: bool) -> Result<(), String> {
    let path = app_dir.join(OFF_FILE);
    let r = if on {
        match std::fs::remove_file(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            r => r,
        }
    } else {
        std::fs::create_dir_all(app_dir).and_then(|()| std::fs::write(&path, b""))
    };
    r.map_err(|e| format!("{}: {e}", path.display()))
}

/// A missing or unreadable file means nothing was told yet: every machine
/// becomes a baseline, so the worst case is one missed alert, never a flood.
fn load_seen(app_dir: &Path) -> Seen {
    std::fs::read_to_string(seen_path(app_dir))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_seen(app_dir: &Path, seen: &Seen) -> Result<(), String> {
    let path = seen_path(app_dir);
    let tmp = app_dir.join(format!("{SEEN_FILE}.tmp"));
    let json = serde_json::to_vec(seen).expect("seen serializes");
    std::fs::create_dir_all(app_dir)
        .and_then(|()| std::fs::write(&tmp, json))
        .and_then(|()| std::fs::rename(&tmp, &path))
        .map_err(|e| format!("{}: {e}", path.display()))
}

fn seen_path(app_dir: &Path) -> PathBuf {
    app_dir.join(SEEN_FILE)
}

fn err(e: patchpilot_store::StoreError) -> String {
    e.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use patchpilot_core::{Manifest, ProductId};

    fn load(rel: &str) -> Manifest {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures")
            .join(rel);
        Manifest::from_json(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    /// The laptop with a plug-in the studio's Golden Rig doesn't have.
    fn laptop_with_delay() -> Manifest {
        let mut m = load("parity/score/laptop.json");
        let mut item = m.items[0].clone();
        item.product_id = Some(ProductId("acme-delay".into()));
        item.version = Some("1.0.0".into());
        m.items.push(item);
        let mut p = m.products[0].clone();
        p.canonical_id = ProductId("acme-delay".into());
        p.name = "Acme Delay".into();
        m.products.push(p);
        m
    }

    fn studio_store() -> (Store, Manifest) {
        let studio = load("parity/score/golden.json");
        let mut store = Store::open_in_memory().unwrap();
        crate::save_scan(&mut store, &studio).unwrap();
        store
            .save_golden(&studio, Some(&studio.machine.id), None, 1)
            .unwrap();
        (store, studio)
    }

    #[test]
    fn a_suggestion_from_the_laptop_becomes_golden_and_the_studio_hears_of_it() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path();
        let (mut store, studio) = studio_store();
        let laptop = laptop_with_delay();
        crate::save_scan(&mut store, &laptop).unwrap();
        let this = Some(studio.machine.id.as_str());

        // First look: a baseline, no alerts.
        assert!(check_alerts(&store, app, this).unwrap().is_empty());

        let v = add_to_golden(&mut store, &laptop.machine.id, &["acme-delay".into()], 2).unwrap();
        assert_eq!(v.version, 2);
        assert_eq!(
            v.note.as_deref(),
            Some("Added Acme Delay from Travel Laptop")
        );
        let g = store.golden(None).unwrap().unwrap();
        assert!(
            g.manifest
                .product(&ProductId("acme-delay".into()))
                .is_some()
        );

        // The studio now lacks Acme Delay: one drift alert, about itself.
        let alerts = check_alerts(&store, app, this).unwrap();
        assert_eq!(alerts.len(), 1, "{alerts:?}");
        assert_eq!(alerts[0].title(), "Studio PC drifted from the Golden Rig");
        assert!(check_alerts(&store, app, this).unwrap().is_empty());
    }

    #[test]
    fn rebaseline_and_switching_off_keep_it_quiet() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path();
        let (mut store, studio) = studio_store();
        let laptop = laptop_with_delay();
        crate::save_scan(&mut store, &laptop).unwrap();
        let this = Some(studio.machine.id.as_str());
        rebaseline(&store, app, this).unwrap();

        // Made here, so it's not news here.
        add_to_golden(&mut store, &laptop.machine.id, &["acme-delay".into()], 2).unwrap();
        rebaseline(&store, app, this).unwrap();
        assert!(check_alerts(&store, app, this).unwrap().is_empty());

        assert!(alerts_enabled(app));
        set_alerts_enabled(app, false).unwrap();
        assert!(!alerts_enabled(app));
        let mut fewer = laptop.clone();
        fewer
            .items
            .retain(|i| i.product_id.as_ref().is_none_or(|p| p.0 != "acme-verb"));
        crate::save_scan(&mut store, &fewer).unwrap();
        assert!(check_alerts(&store, app, this).unwrap().is_empty());
        // Back on: what happened while off isn't replayed.
        set_alerts_enabled(app, true).unwrap();
        set_alerts_enabled(app, true).unwrap();
        assert!(check_alerts(&store, app, this).unwrap().is_empty());
        assert!(!app.join("alerts-seen.json.tmp").exists());
    }

    #[test]
    fn ignored_suggestions_are_left_out() {
        let (mut store, studio) = studio_store();
        let laptop = laptop_with_delay();
        crate::save_scan(&mut store, &laptop).unwrap();
        let this = Some(studio.machine.id.as_str());
        let before = observe(&store, this).unwrap();
        store
            .ignore_suggestion("acme-delay", Some("1.0.0"), 3)
            .unwrap();
        assert_eq!(
            ignored_keys(&store).unwrap(),
            BTreeSet::from(["acme-delay@1.0.0".to_owned()])
        );
        assert_ne!(observe(&store, this).unwrap(), before);
    }
}
