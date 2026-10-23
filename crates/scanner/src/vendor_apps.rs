//! Detect vendor installer apps and licensing tools, and their versions.
//!
//! Detect only. Nothing here reads credentials, license files, account data or
//! caches (CLAUDE.md rule 6): only Windows uninstall entries and macOS app
//! bundle `Info.plist` files.
//!
//! Each vendor is one [`VendorAdapter`] so a vendor renaming its app touches
//! one entry. Install paths and display names are best-known values; confirm
//! against real installs when adding or changing an adapter.

use std::path::{Path, PathBuf};

use patchpilot_core::ItemKind;
use serde::Deserialize;

use crate::plist_info::read_bundle_info;

/// How to recognise one vendor app on each OS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VendorAdapter {
    /// Stable ID used in manifests, e.g. `native-access`.
    pub id: &'static str,
    pub name: &'static str,
    /// [`ItemKind::VendorApp`] or [`ItemKind::LicenseTool`].
    pub kind: ItemKind,
    /// Uninstall `DisplayName`s that match, compared case-insensitively as
    /// prefixes ("Native Access" also matches "Native Access 2").
    pub windows_display_names: &'static [&'static str],
    /// If set, the uninstall `Publisher` must contain this (case-insensitive).
    pub windows_publisher: Option<&'static str>,
    /// Paths relative to a macOS root (`/` or the home folder). The first that
    /// exists wins. `.app` bundles give a version from their `Info.plist`.
    pub macos_paths: &'static [&'static str],
}

pub const ADAPTERS: &[VendorAdapter] = &[
    VendorAdapter {
        id: "native-access",
        name: "Native Access",
        kind: ItemKind::VendorApp,
        windows_display_names: &["Native Access"],
        windows_publisher: Some("Native Instruments"),
        macos_paths: &["Applications/Native Access.app"],
    },
    VendorAdapter {
        id: "waves-central",
        name: "Waves Central",
        kind: ItemKind::VendorApp,
        windows_display_names: &["Waves Central"],
        windows_publisher: Some("Waves"),
        macos_paths: &["Applications/Waves Central.app"],
    },
    VendorAdapter {
        id: "ua-connect",
        name: "UA Connect",
        kind: ItemKind::VendorApp,
        windows_display_names: &["UA Connect"],
        windows_publisher: Some("Universal Audio"),
        macos_paths: &["Applications/UA Connect.app"],
    },
    VendorAdapter {
        id: "arturia-software-center",
        name: "Arturia Software Center",
        kind: ItemKind::VendorApp,
        windows_display_names: &["Arturia Software Center"],
        windows_publisher: Some("Arturia"),
        macos_paths: &["Applications/Arturia Software Center.app"],
    },
    VendorAdapter {
        id: "izotope-product-portal",
        name: "iZotope Product Portal",
        kind: ItemKind::VendorApp,
        windows_display_names: &["iZotope Product Portal"],
        windows_publisher: Some("iZotope"),
        macos_paths: &[
            "Applications/iZotope Product Portal.app",
            "Applications/iZotope/iZotope Product Portal.app",
        ],
    },
    VendorAdapter {
        id: "plugin-alliance-installation-manager",
        name: "Plugin Alliance Installation Manager",
        kind: ItemKind::VendorApp,
        windows_display_names: &["Plugin Alliance Installation Manager"],
        windows_publisher: Some("Plugin Alliance"),
        macos_paths: &["Applications/Plugin Alliance Installation Manager.app"],
    },
    VendorAdapter {
        id: "spitfire-audio",
        name: "Spitfire Audio App",
        kind: ItemKind::VendorApp,
        windows_display_names: &["Spitfire Audio"],
        windows_publisher: Some("Spitfire"),
        macos_paths: &["Applications/Spitfire Audio.app"],
    },
    VendorAdapter {
        id: "ilok-license-manager",
        name: "iLok License Manager",
        kind: ItemKind::LicenseTool,
        windows_display_names: &["iLok License Manager"],
        windows_publisher: Some("PACE"),
        macos_paths: &["Applications/iLok License Manager.app"],
    },
    VendorAdapter {
        id: "pace-license-support",
        name: "PACE License Support",
        kind: ItemKind::LicenseTool,
        windows_display_names: &["PACE License Support"],
        windows_publisher: Some("PACE"),
        // Presence only: the helper has no Info.plist, so no version on macOS.
        macos_paths: &["Library/PrivilegedHelperTools/com.paceap.eden.licensed"],
    },
];

/// One Windows uninstall registry entry (only the values we read).
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct UninstallEntry {
    pub display_name: Option<String>,
    pub display_version: Option<String>,
    pub publisher: Option<String>,
    pub install_location: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Detection {
    Detected {
        version: Option<String>,
        /// Where it was found: an install folder, a bundle path, or the
        /// uninstall entry's name if it gave no location.
        location: Option<String>,
    },
    NotDetected,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VendorAppStatus {
    pub adapter: &'static VendorAdapter,
    pub detection: Detection,
}

/// Match uninstall entries against every adapter. Every adapter gets a
/// status, so missing apps read as "not detected".
pub fn detect_windows(entries: &[UninstallEntry]) -> Vec<VendorAppStatus> {
    ADAPTERS
        .iter()
        .map(|adapter| {
            let found = entries.iter().find(|e| windows_matches(adapter, e));
            let detection = match found {
                Some(e) => Detection::Detected {
                    version: non_empty(e.display_version.as_deref()),
                    location: non_empty(e.install_location.as_deref())
                        .or_else(|| non_empty(e.display_name.as_deref())),
                },
                None => Detection::NotDetected,
            };
            VendorAppStatus { adapter, detection }
        })
        .collect()
}

fn windows_matches(adapter: &VendorAdapter, e: &UninstallEntry) -> bool {
    let Some(name) = e.display_name.as_deref() else {
        return false;
    };
    let name = name.trim().to_lowercase();
    let name_ok = adapter
        .windows_display_names
        .iter()
        .any(|n| name.starts_with(&n.to_lowercase()));
    let publisher_ok = adapter.windows_publisher.is_none_or(|p| {
        e.publisher
            .as_deref()
            .is_some_and(|pub_| pub_.to_lowercase().contains(&p.to_lowercase()))
    });
    name_ok && publisher_ok
}

/// Look for each adapter's macOS paths under `roots` (normally `/` and the
/// home folder; tests pass fixture folders).
pub fn detect_macos(roots: &[PathBuf]) -> Vec<VendorAppStatus> {
    ADAPTERS
        .iter()
        .map(|adapter| {
            let found = roots
                .iter()
                .flat_map(|root| adapter.macos_paths.iter().map(move |p| root.join(p)))
                .find(|p| p.exists());
            let detection = match found {
                Some(path) => Detection::Detected {
                    version: app_version(&path),
                    location: Some(path.to_string_lossy().into_owned()),
                },
                None => Detection::NotDetected,
            };
            VendorAppStatus { adapter, detection }
        })
        .collect()
}

fn app_version(path: &Path) -> Option<String> {
    if !path.is_dir() {
        return None;
    }
    // An unreadable plist still means the app is installed; just no version.
    read_bundle_info(path)
        .ok()
        .flatten()
        .and_then(|b| b.version().map(str::to_owned))
}

fn non_empty(s: Option<&str>) -> Option<String> {
    s.map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

/// Read uninstall entries from the registry: 64-bit and 32-bit views of
/// HKLM, plus HKCU (per-user installs). Read-only.
#[cfg(windows)]
pub fn read_uninstall_entries() -> Vec<UninstallEntry> {
    use winreg::RegKey;
    use winreg::enums::{
        HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY,
    };

    const PATH: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";
    let views = [
        (HKEY_LOCAL_MACHINE, KEY_READ | KEY_WOW64_64KEY),
        (HKEY_LOCAL_MACHINE, KEY_READ | KEY_WOW64_32KEY),
        (HKEY_CURRENT_USER, KEY_READ),
    ];
    let mut out = Vec::new();
    for (hive, flags) in views {
        let Ok(uninstall) = RegKey::predef(hive).open_subkey_with_flags(PATH, flags) else {
            continue;
        };
        for name in uninstall.enum_keys().flatten() {
            let Ok(key) = uninstall.open_subkey_with_flags(&name, flags) else {
                continue;
            };
            let get = |v: &str| key.get_value::<String, _>(v).ok();
            out.push(UninstallEntry {
                display_name: get("DisplayName"),
                display_version: get("DisplayVersion"),
                publisher: get("Publisher"),
                install_location: get("InstallLocation"),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adapter_ids_are_unique() {
        let mut ids: Vec<_> = ADAPTERS.iter().map(|a| a.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), ADAPTERS.len());
    }

    #[test]
    fn publisher_must_match_when_set() {
        let entry = UninstallEntry {
            display_name: Some("Waves Central".into()),
            publisher: Some("Someone Else".into()),
            ..Default::default()
        };
        let waves = ADAPTERS.iter().find(|a| a.id == "waves-central").unwrap();
        assert!(!windows_matches(waves, &entry));
    }
}
