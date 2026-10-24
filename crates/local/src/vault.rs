//! The Installer Vault on this machine: which folder it is, the Downloads
//! folder it collects from, and the installers there waiting to be added.
//!
//! The vault's own format lives in `patchpilot-vault`. This module keeps the
//! per-machine settings in the app-data folder (`vault-settings.json`), since
//! the same vault folder can be shared by several machines (on a NAS) and
//! may sit at a different path on each.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use patchpilot_core::{ItemKind, Manifest, Product};
use patchpilot_vault::{Guess, Index, Vault, guess, installers_in, match_product};
use serde::{Deserialize, Serialize};

const SETTINGS_FILE: &str = "vault-settings.json";

/// This machine's vault settings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VaultSettings {
    /// The vault folder; `None` until the user picks one, and the Downloads
    /// folder isn't watched before then.
    #[serde(default)]
    pub root: Option<PathBuf>,
    /// Downloads the user said aren't for the vault, as [`dismiss_key`]s.
    #[serde(default)]
    pub dismissed: Vec<String>,
}

impl VaultSettings {
    pub fn load(app_dir: &Path) -> Self {
        fs::read_to_string(app_dir.join(SETTINGS_FILE))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    /// Write via a temporary file and rename.
    pub fn save(&self, app_dir: &Path) -> io::Result<()> {
        fs::create_dir_all(app_dir)?;
        let temp = app_dir.join(format!("{SETTINGS_FILE}.partial"));
        fs::write(
            &temp,
            serde_json::to_vec_pretty(self).expect("settings serialize"),
        )?;
        fs::rename(&temp, app_dir.join(SETTINGS_FILE))
    }

    pub fn vault(&self) -> Option<Vault> {
        self.root.clone().map(Vault::new)
    }
}

/// The folder suggested the first time: `Documents/Patch Pilot Vault`
/// (Joshua's choice, 2026-10-24). The user can pick any other.
pub fn suggested_root() -> Option<PathBuf> {
    Some(
        PathBuf::from(crate::home_dir()?)
            .join("Documents")
            .join("Patch Pilot Vault"),
    )
}

/// The user's Downloads folder, if it exists.
pub fn downloads_dir() -> Option<PathBuf> {
    let d = PathBuf::from(crate::home_dir()?).join("Downloads");
    d.is_dir().then_some(d)
}

/// Identifies a download for "not for the vault": its name and size, so a
/// new download with the same name still shows up.
pub fn dismiss_key(name: &str, size: u64) -> String {
    format!("{size}:{name}")
}

/// An installer in Downloads that isn't in the vault yet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InboxItem {
    pub path: PathBuf,
    pub file_name: String,
    pub size: u64,
    /// Unix seconds.
    pub modified: i64,
    pub guess: Guess,
    /// The known product it most likely installs.
    pub product: Option<Product>,
}

/// Installers directly in `downloads` that aren't in `index` (same name and
/// size) and weren't dismissed, newest first.
pub fn inbox(
    downloads: &Path,
    index: &Index,
    dismissed: &[String],
    products: &[Product],
) -> Vec<InboxItem> {
    installers_in(downloads)
        .into_iter()
        .filter_map(|path| inspect(&path, products))
        .filter(|item| {
            let in_vault = index
                .entries
                .iter()
                .any(|e| e.source_name == item.file_name && e.size == item.size);
            !in_vault && !dismissed.contains(&dismiss_key(&item.file_name, item.size))
        })
        .collect()
}

/// What Patch Pilot makes of one installer file; `None` if it isn't a
/// readable installer file.
pub fn inspect(path: &Path, products: &[Product]) -> Option<InboxItem> {
    if !patchpilot_vault::is_installer(path) {
        return None;
    }
    let meta = fs::metadata(path).ok().filter(|m| m.is_file())?;
    let file_name = path.file_name()?.to_string_lossy().into_owned();
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs() as i64);
    Some(InboxItem {
        guess: guess(&file_name),
        product: match_product(&file_name, products).cloned(),
        path: path.to_path_buf(),
        file_name,
        size: meta.len(),
        modified,
    })
}

/// Products an installer could be for: plug-ins, vendor apps and licensing
/// tools in any of `manifests` (templates and folders aren't installed),
/// one per ID, sorted by vendor then name.
pub fn installable_products<'a>(manifests: impl IntoIterator<Item = &'a Manifest>) -> Vec<Product> {
    let mut out: BTreeMap<String, Product> = BTreeMap::new();
    for m in manifests {
        for item in &m.items {
            if !matches!(
                item.kind,
                ItemKind::Plugin | ItemKind::VendorApp | ItemKind::LicenseTool
            ) {
                continue;
            }
            let Some(p) = item.product_id.as_ref().and_then(|id| m.product(id)) else {
                continue;
            };
            out.entry(p.canonical_id.0.clone())
                .or_insert_with(|| p.clone());
        }
    }
    let mut v: Vec<Product> = out.into_values().collect();
    v.sort_by_key(|p| (p.vendor.to_lowercase(), p.name.to_lowercase()));
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use patchpilot_core::ProductId;
    use patchpilot_vault::NewEntry;

    #[test]
    fn settings_round_trip_and_default_to_off() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(VaultSettings::load(dir.path()), VaultSettings::default());
        let s = VaultSettings {
            root: Some(dir.path().join("Vault")),
            dismissed: vec![dismiss_key("a.exe", 3)],
        };
        s.save(dir.path()).unwrap();
        assert_eq!(VaultSettings::load(dir.path()), s);
    }

    #[test]
    fn inbox_skips_vaulted_and_dismissed_downloads() {
        let dir = tempfile::tempdir().unwrap();
        let downloads = dir.path().join("Downloads");
        fs::create_dir_all(&downloads).unwrap();
        for (name, body) in [
            ("FabFilter Pro-Q 3 v3.21 Installer.exe", &b"aaa"[..]),
            ("Diva_145_12345_Mac.zip", b"bb"),
            ("Junk Setup.exe", b"c"),
        ] {
            fs::write(downloads.join(name), body).unwrap();
        }
        let vault = Vault::new(dir.path().join("Vault"));
        vault
            .add(
                &downloads.join("Diva_145_12345_Mac.zip"),
                NewEntry::default(),
                None,
                0,
            )
            .unwrap();
        let products = [Product {
            canonical_id: ProductId("fabfilter/pro-q-3".into()),
            vendor: "FabFilter".into(),
            name: "Pro-Q 3".into(),
        }];
        let items = inbox(
            &downloads,
            &vault.index().unwrap(),
            &[dismiss_key("Junk Setup.exe", 1)],
            &products,
        );
        assert_eq!(items.len(), 1, "{items:?}");
        assert_eq!(items[0].file_name, "FabFilter Pro-Q 3 v3.21 Installer.exe");
        assert_eq!(items[0].guess.version.as_deref(), Some("3.21"));
        assert_eq!(items[0].product.as_ref().unwrap().name, "Pro-Q 3");
    }
}
