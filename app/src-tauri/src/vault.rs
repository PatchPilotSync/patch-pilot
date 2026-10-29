//! Installer Vault (strategy section 13) and opening vendor apps: the
//! commands behind the Vault tab and the buttons in Up To Speed.
//!
//! The Downloads folder is watched (not polled) only once the user has
//! picked a vault folder; new installers there show up in the Vault tab
//! through a `vault-changed` event. Nothing is copied until the user clicks
//! Add, and no installer runs until they click Run.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use patchpilot_core::{ItemKind, Os, Product, ProductId};
use patchpilot_local::open::{Opened, VendorApp};
use patchpilot_local::vault::{InboxItem, VaultSettings, dismiss_key};
use patchpilot_local::{FolderWatch, app_dir};
use patchpilot_vault::{Added, Entry, NewEntry, Vault, best_for};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::compare::existing_store;
use crate::rig::{db_path, err};

/// Browsers rename a finished download in one step, but some write the
/// final name early; wait for the folder to settle.
const DOWNLOADS_QUIET: Duration = Duration::from_secs(3);

#[derive(Default)]
pub struct DownloadsWatch(Mutex<Option<FolderWatch>>);

/// What the Vault tab shows; mirrored by `VaultView` in `src/api.ts`.
#[derive(Debug, Serialize)]
pub struct VaultView {
    /// `None` until a folder is picked.
    pub root: Option<String>,
    pub suggested_root: Option<String>,
    /// Watched for new installers once there's a vault.
    pub downloads: Option<String>,
    pub entries: Vec<EntryView>,
    pub inbox: Vec<InboxItem>,
    /// Downloads the user skipped, so a skip can be undone.
    pub skipped: Vec<InboxItem>,
    /// Products an installer can be filed under.
    pub products: Vec<Product>,
    /// Set when the vault folder can't be read (e.g. a NAS that's offline).
    pub problem: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct EntryView {
    #[serde(flatten)]
    pub entry: Entry,
    /// For this machine's OS (or doesn't say).
    pub fits_here: bool,
    /// The file isn't where the index says (moved, or the drive is out).
    pub missing: bool,
}

#[derive(Debug, Serialize)]
pub struct AddResult {
    pub entry: Entry,
    /// It was already in the vault, so nothing was copied.
    pub already: bool,
    /// Set when an earlier installer of the same product was signed by
    /// someone else: a reason to be careful before running it.
    pub signer_warning: Option<String>,
}

/// An installer in the vault for a product a plan needs; mirrored by
/// `VaultMatch` in `src/api.ts`.
#[derive(Debug, Serialize)]
pub struct VaultMatch {
    pub product_id: ProductId,
    pub entry_id: String,
    pub file_name: String,
    pub version: Option<String>,
}

#[tauri::command]
pub async fn vault_status() -> Result<VaultView, String> {
    tauri::async_runtime::spawn_blocking(view)
        .await
        .map_err(|e| e.to_string())?
}

/// Use `path` as the vault folder (created if needed) and start watching
/// Downloads.
#[tauri::command]
pub async fn vault_set_root(app: AppHandle, path: String) -> Result<VaultView, String> {
    let root = PathBuf::from(path.trim());
    if !root.is_absolute() {
        return Err("Pick a full folder path".to_owned());
    }
    std::fs::create_dir_all(&root).map_err(|e| format!("{}: {e}", root.display()))?;
    let dir = settings_dir()?;
    let mut settings = VaultSettings::load(&dir);
    settings.root = Some(root);
    settings.save(&dir).map_err(|e| e.to_string())?;
    watch_downloads(&app);
    vault_status().await
}

/// Copy an installer into the vault, filed as the user confirmed.
#[tauri::command]
pub async fn vault_add(path: PathBuf, meta: NewEntry) -> Result<AddResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let vault = vault()?;
        let signer = patchpilot_vault::signature::signer(&path);
        let earlier = vault.index().map_err(|e| e.to_string())?;
        let added = vault
            .add(&path, meta, signer, patchpilot_local::now())
            .map_err(|e| e.to_string())?;
        let entry = added.entry().clone();
        let signer_warning = match (&entry.product_id, &entry.signer) {
            (Some(id), Some(now)) => earlier
                .entries
                .iter()
                .filter(|e| e.product_id.as_ref() == Some(id))
                .find_map(|e| e.signer.as_ref().filter(|s| *s != now))
                .map(|before| {
                    format!(
                        "Signed by {now}, but an earlier {} installer was signed by {before}. Check it came from the vendor before running it.",
                        entry.product
                    )
                }),
            _ => None,
        };
        Ok(AddResult {
            already: matches!(added, Added::AlreadyThere(_)),
            entry,
            signer_warning,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Installer files dropped on the window, with Patch Pilot's guess at each.
/// Files that aren't installers are left out.
#[tauri::command]
pub async fn vault_inspect(paths: Vec<PathBuf>) -> Result<Vec<InboxItem>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let products = products()?;
        Ok(paths
            .iter()
            .filter_map(|p| patchpilot_local::vault::inspect(p, &products))
            .collect())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// "Not for the vault" (`skip`), or undo that: hide these downloads from
/// the new-installer list, or bring them back.
#[tauri::command]
pub async fn vault_skip(paths: Vec<PathBuf>, skip: bool) -> Result<(), String> {
    // One change to the settings file at a time, so quick skips don't
    // overwrite each other.
    static SETTINGS_LOCK: Mutex<()> = Mutex::new(());
    let _turn = SETTINGS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = settings_dir()?;
    let mut settings = VaultSettings::load(&dir);
    for path in &paths {
        let meta = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let key = dismiss_key(&name, meta.len());
        settings.dismissed.retain(|k| *k != key);
        if skip {
            settings.dismissed.push(key);
        }
    }
    settings.save(&dir).map_err(|e| e.to_string())
}

/// Take an installer back out of the vault (undo an add). Only Patch
/// Pilot's copy is deleted; the original download stays.
#[tauri::command]
pub async fn vault_remove(entry_id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        vault()?
            .remove(&entry_id)
            .map(drop)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Installers in the vault for `products`, for this machine's OS.
#[tauri::command]
pub async fn vault_matches(products: Vec<String>) -> Result<Vec<VaultMatch>, String> {
    let Some(vault) = VaultSettings::load(&settings_dir()?).vault() else {
        return Ok(Vec::new());
    };
    let Some(os) = Os::current() else {
        return Ok(Vec::new());
    };
    // A vault on a drive that's out reads as "nothing in the vault" here.
    let Ok(index) = vault.index() else {
        return Ok(Vec::new());
    };
    Ok(products
        .into_iter()
        .filter_map(|id| {
            let id = ProductId(id);
            let e = best_for(&index, &id, os)?;
            Some(VaultMatch {
                product_id: id,
                entry_id: e.id.clone(),
                file_name: e.source_name.clone(),
                version: e.version.clone(),
            })
        })
        .collect())
}

/// Open an installer from the vault, as a double-click would, after checking
/// it's still exactly the file that was added.
#[tauri::command]
pub async fn vault_run(entry_id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let vault = vault()?;
        let entry = vault.entry(&entry_id).map_err(|e| e.to_string())?;
        if !vault.verify(&entry).map_err(|e| e.to_string())? {
            return Err(format!(
                "{} has changed since it was added to the vault, so Patch Pilot won't open it.",
                entry.source_name
            ));
        }
        // An installer unpacks to the system drive's temp folder first; if
        // even that won't fit, say so instead of letting it fail halfway.
        patchpilot_local::disks::room_for(&std::env::temp_dir(), entry.size)?;
        patchpilot_local::open::open_path(&vault.path_of(&entry)).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Show an installer (or, with no ID, the vault folder) in Finder / Explorer.
#[tauri::command]
pub async fn vault_reveal(entry_id: Option<String>) -> Result<(), String> {
    let vault = vault()?;
    let result = match entry_id {
        Some(id) => {
            let entry = vault.entry(&id).map_err(|e| e.to_string())?;
            patchpilot_local::open::reveal(&vault.path_of(&entry))
        }
        None => patchpilot_local::open::open_path(&vault.root),
    };
    result.map_err(|e| e.to_string())
}

/// Vendor apps and licensing tools, and whether each can be opened here.
#[tauri::command]
pub async fn vendor_apps() -> Result<Vec<VendorApp>, String> {
    tauri::async_runtime::spawn_blocking(patchpilot_local::open::vendor_apps)
        .await
        .map_err(|e| e.to_string())
}

/// Open a vendor app by adapter ID (e.g. `native-access`), or its download
/// page if it isn't installed.
#[tauri::command]
pub async fn open_vendor_app(id: String) -> Result<Opened, String> {
    tauri::async_runtime::spawn_blocking(move || {
        patchpilot_local::open::open_vendor_app(&id).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Open a vendor's `https://` website, or a web search for a plug-in, in
/// the browser.
#[tauri::command]
pub async fn open_web_page(url: String) -> Result<(), String> {
    patchpilot_local::open::open_url(&url).map_err(|e| e.to_string())
}

/// Start (or restart) watching Downloads, if there's a vault.
pub fn watch_downloads(app: &AppHandle) {
    let state = app.state::<DownloadsWatch>();
    let Ok(mut current) = state.0.lock() else {
        return;
    };
    *current = None;
    let has_vault = settings_dir().is_ok_and(|d| VaultSettings::load(&d).root.is_some());
    let Some(downloads) = patchpilot_local::vault::downloads_dir().filter(|_| has_vault) else {
        return;
    };
    let handle = app.clone();
    *current = patchpilot_local::watch_folder_flat(downloads, DOWNLOADS_QUIET, move || {
        let _ = handle.emit("vault-changed", ());
    })
    .ok();
}

pub fn setup(app: &AppHandle) {
    app.manage(DownloadsWatch::default());
    watch_downloads(app);
}

fn settings_dir() -> Result<PathBuf, String> {
    Ok(app_dir(&db_path()?).to_path_buf())
}

fn vault() -> Result<Vault, String> {
    VaultSettings::load(&settings_dir()?)
        .vault()
        .ok_or_else(|| "Pick a vault folder first".to_owned())
}

fn view() -> Result<VaultView, String> {
    let settings = VaultSettings::load(&settings_dir()?);
    let downloads = patchpilot_local::vault::downloads_dir();
    let mut out = VaultView {
        root: settings.root.as_ref().map(|r| r.display().to_string()),
        suggested_root: patchpilot_local::vault::suggested_root().map(|r| r.display().to_string()),
        downloads: downloads.as_ref().map(|d| d.display().to_string()),
        entries: Vec::new(),
        inbox: Vec::new(),
        skipped: Vec::new(),
        products: products()?,
        problem: None,
    };
    let Some(vault) = settings.vault() else {
        return Ok(out);
    };
    let index = match vault.index() {
        Ok(i) if vault.root.is_dir() => i,
        Ok(_) => {
            out.problem = Some(format!(
                "Can't find the vault folder {}. Is the drive connected?",
                vault.root.display()
            ));
            return Ok(out);
        }
        Err(e) => {
            out.problem = Some(e.to_string());
            return Ok(out);
        }
    };
    let os = Os::current();
    out.entries = index
        .entries
        .iter()
        .map(|e| EntryView {
            fits_here: os.is_some_and(|os| e.fits(os)),
            missing: !vault.path_of(e).is_file(),
            entry: e.clone(),
        })
        .collect();
    if let Some(d) = &downloads {
        out.inbox = patchpilot_local::vault::inbox(d, &index, &settings.dismissed, &out.products);
        out.skipped = patchpilot_local::vault::skipped(d, &settings.dismissed, &out.products);
    }
    Ok(out)
}

/// Every installable product Patch Pilot knows: Primary Rig's and every machine's.
fn products() -> Result<Vec<Product>, String> {
    let Some(store) = existing_store()? else {
        return Ok(Vec::new());
    };
    let mut manifests = Vec::new();
    if let Some(g) = store.golden(None).map_err(err)? {
        manifests.push(g.manifest);
    }
    // Only what can be installed: a full scan is mostly presets and files.
    for m in store.machines().map_err(err)? {
        if let Some((id, _)) = store.latest_snapshot(&m.id).map_err(err)? {
            manifests.push(
                store
                    .snapshot_manifest_of(
                        id,
                        &[ItemKind::Plugin, ItemKind::VendorApp, ItemKind::LicenseTool],
                    )
                    .map_err(err)?,
            );
        }
    }
    Ok(patchpilot_local::vault::installable_products(&manifests))
}
