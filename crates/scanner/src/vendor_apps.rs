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
    /// Bundle names (without `.app`) to look for in `Applications` and the
    /// folders right inside it when none of `macos_paths` exists, compared
    /// case-insensitively as prefixes. A bundle in a subfolder also matches
    /// as `<folder> <name>`, so `Plugin Alliance/Installation Manager.app`
    /// would match "Plugin Alliance Installation Manager".
    pub macos_names: &'static [&'static str],
    /// Executable names inside the Windows install folder, used to open the
    /// app when its uninstall entry has no `DisplayIcon` pointing at it.
    pub windows_exes: &'static [&'static str],
    /// The vendor's page for downloading the app, opened when it isn't
    /// installed. Empty if there is no separate download.
    pub download_url: &'static str,
}

pub const ADAPTERS: &[VendorAdapter] = &[
    VendorAdapter {
        id: "native-access",
        name: "Native Access",
        kind: ItemKind::VendorApp,
        windows_display_names: &["Native Access"],
        windows_publisher: Some("Native Instruments"),
        macos_paths: &["Applications/Native Access.app"],
        macos_names: &["Native Access"],
        windows_exes: &["Native Access.exe"],
        download_url: "https://www.native-instruments.com/en/specials/native-access/",
    },
    VendorAdapter {
        id: "waves-central",
        name: "Waves Central",
        kind: ItemKind::VendorApp,
        windows_display_names: &["Waves Central"],
        windows_publisher: Some("Waves"),
        macos_paths: &["Applications/Waves Central.app"],
        macos_names: &["Waves Central"],
        windows_exes: &["Waves Central.exe"],
        download_url: "https://www.waves.com/downloads/central",
    },
    VendorAdapter {
        id: "ua-connect",
        name: "UA Connect",
        kind: ItemKind::VendorApp,
        windows_display_names: &["UA Connect"],
        windows_publisher: Some("Universal Audio"),
        macos_paths: &["Applications/UA Connect.app"],
        macos_names: &["UA Connect", "Universal Audio Connect"],
        windows_exes: &["UA Connect.exe"],
        download_url: "https://www.uaudio.com/downloads",
    },
    VendorAdapter {
        id: "arturia-software-center",
        name: "Arturia Software Center",
        kind: ItemKind::VendorApp,
        windows_display_names: &["Arturia Software Center"],
        windows_publisher: Some("Arturia"),
        macos_paths: &[
            "Applications/Arturia Software Center.app",
            "Applications/Arturia/Arturia Software Center.app",
        ],
        macos_names: &["Arturia Software Center"],
        windows_exes: &["Arturia Software Center.exe"],
        download_url: "https://www.arturia.com/support/downloads-manuals",
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
        macos_names: &["iZotope Product Portal"],
        windows_exes: &["iZotope Product Portal.exe"],
        download_url: "https://www.izotope.com/en/products/product-portal.html",
    },
    VendorAdapter {
        id: "plugin-alliance-installation-manager",
        name: "Plugin Alliance Installation Manager",
        kind: ItemKind::VendorApp,
        // The app itself is `PA-InstallationManager` on both systems.
        windows_display_names: &[
            "Plugin Alliance Installation Manager",
            "PA-InstallationManager",
            "PA Installation Manager",
        ],
        windows_publisher: Some("Plugin Alliance"),
        macos_paths: &[
            "Applications/PA-InstallationManager.app",
            "Applications/Plugin Alliance Installation Manager.app",
        ],
        macos_names: &[
            "PA-InstallationManager",
            "Plugin Alliance Installation Manager",
            "PA Installation Manager",
        ],
        windows_exes: &[
            "PA-InstallationManager.exe",
            "Plugin Alliance Installation Manager.exe",
        ],
        download_url: "https://www.plugin-alliance.com/en/installation-manager.html",
    },
    VendorAdapter {
        id: "spitfire-audio",
        name: "Spitfire Audio App",
        kind: ItemKind::VendorApp,
        windows_display_names: &["Spitfire Audio"],
        windows_publisher: Some("Spitfire"),
        macos_paths: &["Applications/Spitfire Audio.app"],
        macos_names: &["Spitfire Audio"],
        windows_exes: &["Spitfire Audio.exe"],
        download_url: "https://www.spitfireaudio.com/info/library-manager/",
    },
    VendorAdapter {
        id: "ilok-license-manager",
        name: "iLok License Manager",
        kind: ItemKind::LicenseTool,
        windows_display_names: &["iLok License Manager"],
        windows_publisher: Some("PACE"),
        macos_paths: &["Applications/iLok License Manager.app"],
        macos_names: &["iLok License Manager"],
        windows_exes: &["iLok License Manager.exe"],
        download_url: "https://www.ilok.com/#!license-manager",
    },
    VendorAdapter {
        id: "pace-license-support",
        name: "PACE License Support",
        kind: ItemKind::LicenseTool,
        windows_display_names: &["PACE License Support"],
        windows_publisher: Some("PACE"),
        // The helper has no Info.plist, so it gives presence only. License
        // Support also installs iLok License Manager, whose build number makes
        // the same version as on Windows ("6.0.1 GM (b6847, ...)" → 6.0.1.6847).
        macos_paths: &[
            "Library/PrivilegedHelperTools/com.paceap.eden.licensed",
            "Library/LaunchDaemons/com.paceap.eden.licensed.plist",
            "Applications/iLok License Manager.app",
        ],
        macos_names: &[],
        windows_exes: &[],
        download_url: "https://www.ilok.com/#!license-manager",
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
    /// Often the app's own `.exe`, sometimes with an icon index (`,0`).
    pub display_icon: Option<String>,
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
/// home folder; tests pass fixture folders), then for its bundle names in
/// their `Applications` folders, since vendors move apps into subfolders
/// (`/Applications/Arturia/Arturia Software Center.app`).
pub fn detect_macos(roots: &[PathBuf]) -> Vec<VendorAppStatus> {
    let bundles = app_bundles(roots);
    ADAPTERS
        .iter()
        .map(|adapter| {
            let found = roots
                .iter()
                .flat_map(|root| adapter.macos_paths.iter().map(move |p| root.join(p)))
                .find(|p| p.exists())
                .or_else(|| {
                    bundles
                        .iter()
                        .find(|b| b.matches(adapter.macos_names))
                        .map(|b| b.path.clone())
                });
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

/// The app's `.exe` on Windows: the uninstall entry's `DisplayIcon` when it
/// is an existing `.exe`, else one of the adapter's `windows_exes` in its
/// install folder.
pub fn windows_executable(adapter: &VendorAdapter, entries: &[UninstallEntry]) -> Option<PathBuf> {
    let e = entries.iter().find(|e| windows_matches(adapter, e))?;
    let icon = e.display_icon.as_deref().map(|i| {
        let i = i.trim();
        // `"C:\...\App.exe",0` → `C:\...\App.exe`
        let i = match i.rsplit_once(',') {
            Some((path, n)) if n.trim().parse::<i32>().is_ok() => path,
            _ => i,
        };
        PathBuf::from(i.trim().trim_matches('"'))
    });
    let from_icon = icon
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("exe")) && p.is_file());
    from_icon.or_else(|| {
        let dir = PathBuf::from(non_empty(e.install_location.as_deref())?.trim_matches('"'));
        adapter
            .windows_exes
            .iter()
            .map(|x| dir.join(x))
            .find(|p| p.is_file())
    })
}

/// An `.app` bundle in an `Applications` folder, with the lowercase names it
/// answers to: its own, and `<folder> <name>` when it's in a subfolder.
struct Bundle {
    path: PathBuf,
    names: Vec<String>,
}

impl Bundle {
    fn matches(&self, wanted: &[&str]) -> bool {
        wanted.iter().any(|w| {
            let w = w.to_lowercase();
            self.names.iter().any(|n| n.starts_with(&w))
        })
    }
}

/// `.app` bundles in each root's `Applications` folder and the folders right
/// inside it, in root order then by name. Reads folder listings only.
fn app_bundles(roots: &[PathBuf]) -> Vec<Bundle> {
    let mut out = Vec::new();
    for root in roots {
        let apps = root.join("Applications");
        for entry in sorted_entries(&apps) {
            let Some(name) = file_name(&entry) else {
                continue;
            };
            if let Some(stem) = app_stem(&name) {
                out.push(Bundle {
                    names: vec![stem.to_lowercase()],
                    path: entry,
                });
            } else if entry.is_dir() && !name.starts_with('.') {
                for inner in sorted_entries(&entry) {
                    let Some(stem) =
                        file_name(&inner).and_then(|n| app_stem(&n).map(str::to_owned))
                    else {
                        continue;
                    };
                    out.push(Bundle {
                        names: vec![stem.to_lowercase(), format!("{name} {stem}").to_lowercase()],
                        path: inner,
                    });
                }
            }
        }
    }
    out
}

fn sorted_entries(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| rd.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    v.sort();
    v
}

fn file_name(p: &Path) -> Option<String> {
    p.file_name().map(|n| n.to_string_lossy().into_owned())
}

/// `Native Access` for `Native Access.app`; None for anything else.
fn app_stem(name: &str) -> Option<&str> {
    let (stem, ext) = name.rsplit_once('.')?;
    (ext.eq_ignore_ascii_case("app") && !stem.is_empty()).then_some(stem)
}

fn app_version(path: &Path) -> Option<String> {
    if !path.is_dir() {
        return None;
    }
    // An unreadable plist still means the app is installed; just no version.
    read_bundle_info(path)
        .ok()
        .flatten()
        .and_then(|b| b.version().map(build_version))
}

/// PACE writes `6.0.1 GM (b6847, f802503d)` on macOS where Windows says
/// `6.0.1.6847`; turn the first into the second so they compare. Anything
/// else is kept as is.
fn build_version(v: &str) -> String {
    let tidy = || {
        let (version, rest) = v.trim().split_once(' ')?;
        let build = rest.split_once("(b")?.1;
        let digits: String = build.chars().take_while(char::is_ascii_digit).collect();
        let numeric = version
            .split('.')
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()));
        (numeric && !digits.is_empty()).then(|| format!("{version}.{digits}"))
    };
    tidy().unwrap_or_else(|| v.to_owned())
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
                display_icon: get("DisplayIcon"),
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
    fn pace_build_numbers_read_like_windows_versions() {
        assert_eq!(build_version("6.0.1 GM (b6847, f802503d)"), "6.0.1.6847");
        assert_eq!(build_version("2.12.0"), "2.12.0");
        assert_eq!(build_version("Version 3 (beta)"), "Version 3 (beta)");
    }

    #[test]
    fn app_stems() {
        assert_eq!(app_stem("Native Access.app"), Some("Native Access"));
        assert_eq!(app_stem("Native Access.APP"), Some("Native Access"));
        assert_eq!(app_stem("Arturia"), None);
        assert_eq!(app_stem("notes.txt"), None);
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

    #[test]
    fn finds_the_windows_exe_from_the_icon_or_the_install_folder() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("Native Access.exe");
        std::fs::write(&exe, b"MZ").unwrap();
        let na = ADAPTERS.iter().find(|a| a.id == "native-access").unwrap();
        let entry = |icon: Option<String>, location: Option<String>| UninstallEntry {
            display_name: Some("Native Access 2".into()),
            publisher: Some("Native Instruments".into()),
            display_icon: icon,
            install_location: location,
            ..Default::default()
        };
        let icon = format!("\"{}\",0", exe.display());
        assert_eq!(
            windows_executable(na, &[entry(Some(icon), None)]),
            Some(exe.clone())
        );
        let folder = dir.path().to_string_lossy().into_owned();
        assert_eq!(
            windows_executable(na, &[entry(Some("C:\\missing.ico".into()), Some(folder))]),
            Some(exe)
        );
        assert_eq!(windows_executable(na, &[entry(None, None)]), None);
    }
}
