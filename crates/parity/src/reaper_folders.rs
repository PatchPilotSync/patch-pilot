//! Reaper's folder settings, and pointing them at the right folders on
//! another machine.
//!
//! `reaper.ini` keeps absolute paths: the recording, project and render
//! folders, the peaks folder, and the Media Explorer's folder shortcuts
//! (often the sample library). The Golden Rig might say `D:\Samples\Drums`
//! where this Mac has `/Volumes/Samples/Drums`. Each setting travels in the
//! manifest as a `config` item whose path is the setting's folder, made
//! portable with the machine's path map (`HOME`, `REAPER_RESOURCES`, ...).
//! On another machine it resolves through that machine's map, or, for a
//! folder outside every root, by looking for the same folder names on this
//! machine's drives and volumes.

use patchpilot_core::{
    Confidence, Item, ItemKind, Manifest, Os, PathMap, PortablePath, Product, ProductId,
};
use serde::Serialize;

use crate::{ini, reaper::slug};

const PREFIX: &str = "reaper-folder/";
const EXPLORER_PREFIX: &str = "reaper-folder/explorer/";
const EXPLORER: &str = "reaper_explorer";

/// `[REAPER]` keys holding one folder, with the name shown for them.
pub const FOLDER_KEYS: &[(&str, &str)] = &[
    ("defrecpath", "Recording folder"),
    ("defsavepath", "New projects folder"),
    ("defrenderpath", "Render folder"),
    ("altpeakspath", "Peaks folder"),
];

/// One folder setting in `reaper.ini`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FolderSetting {
    pub setting: Setting,
    /// The folder, as this machine's OS spells it.
    pub folder: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Setting {
    /// A `[REAPER]` key from [`FOLDER_KEYS`].
    Key { key: String },
    /// A Media Explorer shortcut, matched across machines by its label (its
    /// title, else its folder's name).
    Explorer { label: String },
}

impl Setting {
    pub fn name(&self) -> String {
        match self {
            Self::Key { key } => FOLDER_KEYS
                .iter()
                .find(|(k, _)| k == key)
                .map_or_else(|| key.clone(), |(_, n)| (*n).to_owned()),
            Self::Explorer { label } => format!("Media Explorer: {label}"),
        }
    }

    fn product_id(&self) -> ProductId {
        ProductId(match self {
            Self::Key { key } => format!("{PREFIX}{key}"),
            Self::Explorer { label } => format!("{EXPLORER_PREFIX}{}", slug(label)),
        })
    }
}

/// Folder settings in `reaper.ini` text. Empty values are skipped.
pub fn read_folder_settings(text: &str) -> Vec<FolderSetting> {
    let mut out: Vec<FolderSetting> = FOLDER_KEYS
        .iter()
        .filter_map(|(key, _)| {
            let v = ini::get(text, "REAPER", key)?;
            (!v.is_empty()).then(|| FolderSetting {
                setting: Setting::Key {
                    key: (*key).to_owned(),
                },
                folder: v.to_owned(),
            })
        })
        .collect();
    for (_, folder, title) in explorer_shortcuts(text) {
        let label = title.unwrap_or_else(|| last_component(&folder).to_owned());
        if out
            .iter()
            .any(|s| matches!(&s.setting, Setting::Explorer { label: l } if l.eq_ignore_ascii_case(&label)))
        {
            continue;
        }
        out.push(FolderSetting {
            setting: Setting::Explorer { label },
            folder,
        });
    }
    out
}

/// `(index, folder, title)` of each Media Explorer shortcut.
fn explorer_shortcuts(text: &str) -> Vec<(usize, String, Option<String>)> {
    let lines = ini::section(text, EXPLORER);
    let index = |k: &str, prefix: &str| -> Option<usize> {
        let rest = k
            .get(..prefix.len())?
            .eq_ignore_ascii_case(prefix)
            .then(|| &k[prefix.len()..])?;
        rest.parse().ok()
    };
    let mut out: Vec<_> = lines
        .iter()
        .filter_map(|(k, v)| {
            let n = index(k, "Shortcut")?;
            let title = lines
                .iter()
                .find(|(tk, _)| index(tk, "ShortcutT") == Some(n))
                .map(|(_, t)| (*t).to_owned())
                .filter(|t| !t.is_empty());
            (!v.is_empty()).then(|| (n, (*v).to_owned(), title))
        })
        .collect();
    out.sort_by_key(|(n, ..)| *n);
    out
}

fn last_component(p: &str) -> &str {
    p.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(p)
}

/// Manifest products and items for folder settings, with paths made portable
/// through `map`.
pub fn folder_items(settings: &[FolderSetting], map: &PathMap) -> (Vec<Product>, Vec<Item>) {
    settings
        .iter()
        .map(|s| {
            let id = s.setting.product_id();
            let product = Product {
                canonical_id: id.clone(),
                vendor: "REAPER".to_owned(),
                name: s.setting.name(),
            };
            let item = Item {
                kind: ItemKind::Config,
                product_id: Some(id),
                format: None,
                platform: map.os,
                arch: None,
                version: None,
                path: map.to_portable(&s.folder),
                content_hash: None,
                size: None,
                mtime: None,
                // The setting itself means the same on every OS.
                platform_portable: true,
                confidence: Confidence::High,
                requires: Vec::new(),
            };
            (product, item)
        })
        .unzip()
}

pub fn is_reaper_folder(id: &ProductId) -> bool {
    id.0.starts_with(PREFIX)
}

/// Where this machine can look for a folder that isn't under a mapped root.
pub struct LocalFolders<'a> {
    /// Drive roots (`D:\`), volumes (`/Volumes/Samples`), the home folder.
    pub bases: Vec<String>,
    pub exists: &'a dyn Fn(&str) -> bool,
}

/// One folder setting that doesn't match the Golden Rig.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FolderFix {
    pub setting: Setting,
    pub name: String,
    /// The folder on the Golden Rig, as its OS spells it.
    pub golden: String,
    /// This machine's setting; `None` when it isn't set here.
    pub current: Option<String>,
    /// The matching folder here; `None` when none was found.
    pub proposed: Option<String>,
}

/// Golden's folder settings that differ on `machine`, Golden's order.
pub fn folder_plan(golden: &Manifest, machine: &Manifest, local: &LocalFolders) -> Vec<FolderFix> {
    let settings = |m: &Manifest| -> Vec<(Setting, PortablePath)> {
        m.items
            .iter()
            .filter_map(|i| {
                let id = i.product_id.as_ref()?;
                let setting = setting_of(id, m)?;
                Some((setting, i.path.clone()))
            })
            .collect()
    };
    let here = settings(machine);
    let mut out = Vec::new();
    for (setting, path) in settings(golden) {
        let Some(golden_folder) = golden.machine.path_map.resolve(&path) else {
            continue;
        };
        let current = here
            .iter()
            .find(|(s, _)| s.product_id() == setting.product_id())
            .and_then(|(_, p)| machine.machine.path_map.resolve(p));
        let proposed = remap(
            &path,
            &golden_folder,
            golden.machine.os,
            &machine.machine.path_map,
            local,
        );
        let matches = matches!((&current, &proposed), (Some(c), Some(p)) if same_folder(c, p));
        if !matches {
            out.push(FolderFix {
                name: setting.name(),
                setting,
                golden: golden_folder,
                current,
                proposed,
            });
        }
    }
    out
}

fn setting_of(id: &ProductId, m: &Manifest) -> Option<Setting> {
    if let Some(key) =
        id.0.strip_prefix(PREFIX)
            .filter(|k| !k.starts_with("explorer/"))
    {
        return Some(Setting::Key {
            key: key.to_owned(),
        });
    }
    id.0.strip_prefix(EXPLORER_PREFIX)?;
    let name = &m.product(id)?.name;
    let label = name.strip_prefix("Media Explorer: ").unwrap_or(name);
    Some(Setting::Explorer {
        label: label.to_owned(),
    })
}

/// The folder on this machine matching Golden's `path` (`golden_folder` as
/// Golden spells it).
fn remap(
    path: &PortablePath,
    golden_folder: &str,
    golden_os: Os,
    here: &PathMap,
    local: &LocalFolders,
) -> Option<String> {
    if let PortablePath::Logical { root, .. } = path
        && here.roots.contains_key(root)
    {
        return here.resolve(path);
    }
    let parts = components(golden_folder, golden_os);
    let sep = here.os.separator();
    for start in 0..parts.len() {
        for base in &local.bases {
            let mut candidate = base.trim_end_matches(['/', '\\']).to_owned();
            for p in &parts[start..] {
                candidate.push(sep);
                candidate.push_str(p);
            }
            if (local.exists)(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

/// Folder names below the drive (`D:`) or volume (`/Volumes/<name>`).
fn components(folder: &str, os: Os) -> Vec<&str> {
    let mut parts: Vec<&str> = folder
        .split(['/', '\\'])
        .filter(|p| !p.is_empty())
        .collect();
    match os {
        Os::Windows if parts.first().is_some_and(|p| p.ends_with(':')) => {
            parts.remove(0);
        }
        Os::Macos if parts.first() == Some(&"Volumes") && parts.len() > 1 => {
            parts.drain(..2);
        }
        _ => {}
    }
    parts
}

fn same_folder(a: &str, b: &str) -> bool {
    let norm = |p: &str| {
        p.replace('\\', "/")
            .trim_end_matches('/')
            .to_ascii_lowercase()
    };
    norm(a) == norm(b)
}

/// `reaper.ini` with each fix that has a proposed folder applied.
pub fn apply_folder_fixes(text: &str, fixes: &[FolderFix]) -> String {
    let mut keys = Vec::new();
    let mut explorer = Vec::new();
    let shortcuts = explorer_shortcuts(text);
    let mut next = shortcuts.iter().map(|(n, ..)| n + 1).max().unwrap_or(0);
    for fix in fixes {
        let Some(folder) = &fix.proposed else {
            continue;
        };
        match &fix.setting {
            Setting::Key { key } => keys.push((key.clone(), folder.clone())),
            Setting::Explorer { label } => {
                let existing = shortcuts.iter().find(|(_, f, t)| {
                    t.as_deref()
                        .unwrap_or_else(|| last_component(f))
                        .eq_ignore_ascii_case(label)
                });
                match existing {
                    Some((n, ..)) => explorer.push((format!("Shortcut{n}"), folder.clone())),
                    None => {
                        explorer.push((format!("Shortcut{next}"), folder.clone()));
                        explorer.push((format!("ShortcutT{next}"), label.clone()));
                        next += 1;
                    }
                }
            }
        }
    }
    let mut out = text.to_owned();
    if !keys.is_empty() {
        out = ini::set(&out, "REAPER", &keys);
    }
    if !explorer.is_empty() {
        out = ini::set(&out, EXPLORER, &explorer);
    }
    out
}

#[cfg(test)]
mod tests {
    use patchpilot_core::{Arch, MachineInfo};

    use super::*;

    const WIN_INI: &str = "[REAPER]\r\ndefrecpath=C:\\Users\\josh\\Documents\\REAPER Media\r\naltpeakspath=D:\\Peaks\r\n\r\n[reaper_explorer]\r\nShortcut0=D:\\Samples\\Drums\r\nShortcutT0=Drums\r\nShortcut1=D:\\Loops\r\n";
    const MAC_INI: &str = "[REAPER]\ndefrecpath=/Users/josh/Music/Old\n\n[reaper_explorer]\nShortcut0=/Volumes/Old/Drums\nShortcutT0=Drums\n";

    fn manifest(os: Os, map: PathMap, ini: &str) -> Manifest {
        let mut m = Manifest::new(
            MachineInfo {
                id: format!("{os:?}"),
                name: "M".into(),
                os,
                arch: Arch::X86_64,
                os_version: None,
                path_map: map.clone(),
            },
            0,
        );
        let (p, i) = folder_items(&read_folder_settings(ini), &map);
        m.products = p;
        m.items = i;
        m
    }

    fn win() -> Manifest {
        manifest(
            Os::Windows,
            PathMap::new(Os::Windows).with_root("HOME", r"C:\Users\josh"),
            WIN_INI,
        )
    }

    fn mac() -> Manifest {
        manifest(
            Os::Macos,
            PathMap::new(Os::Macos).with_root("HOME", "/Users/josh"),
            MAC_INI,
        )
    }

    #[test]
    fn reads_folder_keys_and_explorer_shortcuts() {
        let s = read_folder_settings(WIN_INI);
        let names: Vec<_> = s.iter().map(|s| s.setting.name()).collect();
        assert_eq!(
            names,
            [
                "Recording folder",
                "Peaks folder",
                "Media Explorer: Drums",
                "Media Explorer: Loops"
            ]
        );
        assert_eq!(s[3].folder, r"D:\Loops");
    }

    #[test]
    fn home_folders_follow_the_home_root() {
        let m = win();
        let rec = m
            .items
            .iter()
            .find(|i| i.product_id.as_ref().unwrap().0 == "reaper-folder/defrecpath");
        assert_eq!(
            rec.unwrap().path,
            PortablePath::logical("HOME", "Documents/REAPER Media")
        );
    }

    #[test]
    fn plans_fixes_by_root_and_by_folder_names() {
        let exists = |p: &str| p == "/Volumes/Samples/Drums" || p == "/Volumes/Samples/Loops";
        let local = LocalFolders {
            bases: vec!["/".into(), "/Volumes/Samples".into(), "/Users/josh".into()],
            exists: &exists,
        };
        let fixes = folder_plan(&win(), &mac(), &local);
        let by_name = |n: &str| fixes.iter().find(|f| f.name == n).unwrap();

        let rec = by_name("Recording folder");
        assert_eq!(rec.current.as_deref(), Some("/Users/josh/Music/Old"));
        assert_eq!(
            rec.proposed.as_deref(),
            Some("/Users/josh/Documents/REAPER Media")
        );

        let drums = by_name("Media Explorer: Drums");
        assert_eq!(drums.proposed.as_deref(), Some("/Volumes/Samples/Drums"));
        let loops = by_name("Media Explorer: Loops");
        assert_eq!(loops.current, None);
        assert_eq!(loops.proposed.as_deref(), Some("/Volumes/Samples/Loops"));

        let peaks = by_name("Peaks folder");
        assert_eq!(peaks.golden, r"D:\Peaks");
        assert_eq!(peaks.proposed, None, "nothing like it on this Mac");
    }

    #[test]
    fn applied_fixes_leave_nothing_to_do() {
        let exists = |p: &str| ["/Volumes/Samples/Drums", "/Volumes/Samples/Loops"].contains(&p);
        let local = LocalFolders {
            bases: vec!["/Volumes/Samples".into()],
            exists: &exists,
        };
        let fixes = folder_plan(&win(), &mac(), &local);
        let fixed = apply_folder_fixes(MAC_INI, &fixes);
        assert!(fixed.contains("Shortcut0=/Volumes/Samples/Drums\n"));
        assert!(fixed.contains("Shortcut1=/Volumes/Samples/Loops\nShortcutT1=Loops\n"));
        assert!(fixed.contains("defrecpath=/Users/josh/Documents/REAPER Media\n"));

        let after = manifest(
            Os::Macos,
            PathMap::new(Os::Macos).with_root("HOME", "/Users/josh"),
            &fixed,
        );
        let left = folder_plan(&win(), &after, &local);
        assert_eq!(left.len(), 1, "{left:?}");
        assert_eq!(left[0].name, "Peaks folder");
    }

    #[test]
    fn same_os_finds_the_folder_on_another_drive() {
        let other = manifest(
            Os::Windows,
            PathMap::new(Os::Windows).with_root("HOME", r"C:\Users\sam"),
            "[REAPER]\r\n",
        );
        let exists = |p: &str| p == r"E:\Samples\Drums";
        let local = LocalFolders {
            bases: vec![r"C:\".into(), r"E:\".into()],
            exists: &exists,
        };
        let fixes = folder_plan(&win(), &other, &local);
        let drums = fixes
            .iter()
            .find(|f| f.name == "Media Explorer: Drums")
            .unwrap();
        assert_eq!(drums.proposed.as_deref(), Some(r"E:\Samples\Drums"));
        let rec = fixes.iter().find(|f| f.name == "Recording folder").unwrap();
        assert_eq!(
            rec.proposed.as_deref(),
            Some(r"C:\Users\sam\Documents\REAPER Media")
        );
    }
}
