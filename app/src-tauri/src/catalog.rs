//! Plug-in catalog and update alerts (decision #44).
//!
//! The app starts with the catalog compiled into it, then uses the copy
//! cached in app data, then checks the public releases repo for a newer one:
//! at launch and once a day after that (a sleeping timer, no polling of the
//! disk or the network in between). A newer catalog is cached and open windows
//! refresh through `data-changed`.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use patchpilot_catalog::Catalog;
use patchpilot_parity::{Elsewhere, UpdateAlert, UpdateSource, update_alerts};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::rig::{db_path, err, open};

/// The catalog file on the public releases repo (decision #44).
const DEFAULT_CATALOG_URL: &str =
    "https://raw.githubusercontent.com/PatchPilotSync/patch-pilot-releases/main/catalog.json";
const CACHE_FILE: &str = "catalog.json";
const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);
/// Catalogs are small; refuse anything that clearly isn't one.
const MAX_BYTES: usize = 4 * 1024 * 1024;

/// What `update_alerts` returns. Mirrored in `src/api.ts`.
#[derive(Debug, Serialize)]
pub struct UpdatesView {
    pub alerts: Vec<UpdateAlert>,
    /// `YYYY-MM-DD` of the catalog in use.
    pub catalog_updated: String,
}

/// Plug-ins on this machine with a newer version on another machine, in the
/// latest Golden Rig, or in the catalog.
#[tauri::command]
pub async fn updates() -> Result<UpdatesView, String> {
    let catalog = patchpilot_catalog::current();
    let catalog_updated = catalog.updated().to_owned();
    let empty = || UpdatesView {
        alerts: Vec::new(),
        catalog_updated: catalog_updated.clone(),
    };
    let db = db_path()?;
    if !db.exists() {
        return Ok(empty());
    }
    let store = open(&db)?;
    let Some(this) = patchpilot_store::read_machine_id(patchpilot_local::app_dir(&db))
        .map_err(|e| e.to_string())?
    else {
        return Ok(empty());
    };
    let Some(here) = store.latest_manifest(&this).map_err(err)? else {
        return Ok(empty());
    };
    let mut others = Vec::new();
    for info in store.machines().map_err(err)? {
        if info.id == this || info.id.starts_with("golden@") {
            continue;
        }
        if let Some(m) = store.latest_manifest(&info.id).map_err(err)? {
            others.push((UpdateSource::Machine { name: info.name }, m));
        }
    }
    if let Some(g) = store.golden(None).map_err(err)? {
        others.push((
            UpdateSource::Golden {
                version: g.info.version,
            },
            g.manifest,
        ));
    }
    let elsewhere: Vec<Elsewhere<'_>> = others
        .iter()
        .map(|(source, manifest)| Elsewhere { source, manifest })
        .collect();
    Ok(UpdatesView {
        alerts: update_alerts(&here, &elsewhere, &catalog),
        catalog_updated,
    })
}

/// Load the cached catalog, then check for a newer one now and once a day.
pub fn setup(app: &AppHandle) {
    let cache = match db_path() {
        Ok(db) => patchpilot_local::app_dir(&db).join(CACHE_FILE),
        Err(_) => return,
    };
    if let Some(c) = read_cached(&cache) {
        patchpilot_catalog::install(c);
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let http = reqwest::Client::new();
        loop {
            if is_stale(&cache) {
                match fetch(&http).await {
                    Ok(json) => {
                        if let Ok(c) = Catalog::from_json(&json) {
                            let _ = write_cache(&cache, &json);
                            if patchpilot_catalog::install(c) {
                                let _ = app.emit("data-changed", ());
                            }
                        }
                    }
                    // Offline or not published yet: the catalog in use stays.
                    Err(e) => eprintln!("catalog check: {e}"),
                }
            }
            tokio::time::sleep(CHECK_EVERY).await;
        }
    });
}

fn read_cached(path: &Path) -> Option<Catalog> {
    Catalog::from_json(&std::fs::read_to_string(path).ok()?).ok()
}

/// True when the cache is missing or older than a day.
fn is_stale(path: &Path) -> bool {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .is_none_or(|age| age >= CHECK_EVERY - Duration::from_secs(60))
}

async fn fetch(http: &reqwest::Client) -> Result<String, String> {
    let res = http
        .get(catalog_url())
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !res.status().is_success() {
        return Err(format!("{}", res.status()));
    }
    let bytes = res.bytes().await.map_err(|e| e.to_string())?;
    if bytes.len() > MAX_BYTES {
        return Err("catalog too large".to_owned());
    }
    String::from_utf8(bytes.to_vec()).map_err(|e| e.to_string())
}

/// Staged, then renamed into place (CLAUDE.md rule 4 applies to our own files too).
fn write_cache(path: &Path, json: &str) -> std::io::Result<()> {
    let tmp: PathBuf = path.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)
}

fn catalog_url() -> String {
    std::env::var("PATCHPILOT_CATALOG_URL")
        .ok()
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| DEFAULT_CATALOG_URL.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_cache_is_stale_and_a_fresh_one_is_not() {
        let dir = std::env::temp_dir().join(format!("pp-catalog-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(CACHE_FILE);
        let _ = std::fs::remove_file(&path);
        assert!(is_stale(&path));
        let json = serde_json::to_string(Catalog::bundled().file()).unwrap();
        write_cache(&path, &json).unwrap();
        assert!(!is_stale(&path));
        assert_eq!(
            read_cached(&path).unwrap().updated(),
            Catalog::bundled().updated()
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
