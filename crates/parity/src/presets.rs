//! Third-party preset folders (roadmap M10-12, decision #49).
//!
//! Each known preset folder that exists on a machine becomes one
//! [`ItemKind::Folder`] item under its own logical root (`PRESETS_FABFILTER`
//! -> `~/Documents/FabFilter/Presets`), so a Mac and a PC compare folder to
//! folder. The item's content hash covers the files' relative paths and
//! contents, so the comparison can say "your Serum presets differ" without
//! listing thousands of files. Nothing is copied or written.
//!
//! Two kinds of location:
//! - vendor folders from [`LOCATIONS`], for plug-ins that keep presets in
//!   their own place;
//! - the standard preset folders (`~/Library/Audio/Presets` for AU and VST3 on
//!   macOS, `Documents\VST3 Presets` on Windows): every vendor subfolder there
//!   is its own item, per OS, since the two hold different file types.
//!
//! Paths come from vendors' documentation; a folder that doesn't exist is
//! simply skipped.

use std::path::{Path, PathBuf};

use patchpilot_core::{Confidence, Item, ItemKind, Os, PortablePath, Product, ProductId};

/// Product IDs of preset folders start with this.
pub const PRODUCT_PREFIX: &str = "presets/";
/// Files larger than this count by name and size only, not content.
const HASH_LIMIT: u64 = 1024 * 1024;
/// Past this many files a folder counts by name and size only.
const MAX_HASHED_FILES: usize = 5000;
/// Never walk more files than this in one folder.
const MAX_FILES: usize = 50_000;

/// Where a preset folder lives, relative to a per-user or shared base.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Base {
    /// `~/Documents` (both OSes).
    Documents,
    /// Windows `%APPDATA%` (Roaming).
    AppData,
    /// Windows `%ProgramData%`.
    ProgramData,
    /// macOS `~/Library`.
    UserLibrary,
    /// macOS `/Library`.
    Library,
}

#[derive(Debug, Clone, Copy)]
pub struct PresetLocation {
    /// Slug, unique, used in the product ID and the logical root.
    pub id: &'static str,
    pub vendor: &'static str,
    /// What the folder holds, e.g. "Serum presets".
    pub name: &'static str,
    pub macos: Option<(Base, &'static str)>,
    pub windows: Option<(Base, &'static str)>,
    /// True when both OSes lay the folder out the same way, so a Mac and a PC
    /// should hold the same files.
    pub portable: bool,
}

const fn both(
    id: &'static str,
    vendor: &'static str,
    name: &'static str,
    rel: &'static str,
) -> PresetLocation {
    PresetLocation {
        id,
        vendor,
        name,
        macos: Some((Base::Documents, rel)),
        windows: Some((Base::Documents, rel)),
        portable: true,
    }
}

/// Vendor preset folders. Keep sorted by vendor.
pub const LOCATIONS: &[PresetLocation] = &[
    PresetLocation {
        id: "arturia",
        vendor: "Arturia",
        name: "Arturia presets",
        macos: Some((Base::Library, "Arturia/Presets")),
        windows: Some((Base::ProgramData, "Arturia/Presets")),
        portable: true,
    },
    both(
        "fabfilter",
        "FabFilter",
        "FabFilter presets",
        "FabFilter/Presets",
    ),
    both(
        "ik-multimedia",
        "IK Multimedia",
        "IK Multimedia presets",
        "IK Multimedia",
    ),
    both("izotope", "iZotope", "iZotope presets", "iZotope"),
    both(
        "kilohearts",
        "Kilohearts",
        "Kilohearts presets",
        "Kilohearts",
    ),
    both(
        "native-instruments",
        "Native Instruments",
        "Native Instruments User Content",
        "Native Instruments/User Content",
    ),
    PresetLocation {
        id: "spectrasonics",
        vendor: "Spectrasonics",
        name: "Omnisphere user patches",
        macos: Some((
            Base::UserLibrary,
            "Application Support/Spectrasonics/STEAM/Omnisphere/Settings Library/Patches/User",
        )),
        windows: Some((
            Base::ProgramData,
            "Spectrasonics/STEAM/Omnisphere/Settings Library/Patches/User",
        )),
        portable: true,
    },
    both(
        "surge-xt",
        "Surge Synth Team",
        "Surge XT user data",
        "Surge XT",
    ),
    PresetLocation {
        id: "tal",
        vendor: "TAL Software",
        name: "TAL presets",
        macos: Some((Base::UserLibrary, "Application Support/ToguAudioLine")),
        windows: Some((Base::AppData, "ToguAudioLine")),
        portable: true,
    },
    PresetLocation {
        id: "u-he",
        vendor: "u-he",
        name: "u-he presets",
        macos: Some((Base::UserLibrary, "Audio/Presets/u-he")),
        windows: Some((Base::Documents, "u-he")),
        // Windows keeps them in `<Product>.data/Presets`, macOS in `<Product>`.
        portable: false,
    },
    PresetLocation {
        id: "valhalla-dsp",
        vendor: "Valhalla DSP",
        name: "Valhalla presets",
        macos: Some((Base::UserLibrary, "Application Support/Valhalla DSP, LLC")),
        windows: Some((Base::AppData, "Valhalla DSP, LLC")),
        portable: true,
    },
    both("vital", "Vital Audio", "Vital presets", "Vital"),
    both(
        "serum",
        "Xfer Records",
        "Serum presets",
        "Xfer/Serum Presets",
    ),
    both(
        "serum-2",
        "Xfer Records",
        "Serum 2 presets",
        "Xfer/Serum 2 Presets",
    ),
];

/// The standard preset folder whose vendor subfolders each become an item.
fn standard_folder(os: Os) -> (Base, &'static str) {
    match os {
        Os::Macos => (Base::UserLibrary, "Audio/Presets"),
        Os::Windows => (Base::Documents, "VST3 Presets"),
    }
}

/// This machine's base folders. `None` where unknown or not on this OS.
#[derive(Debug, Clone, Default)]
pub struct PresetBases {
    pub documents: Option<PathBuf>,
    pub app_data: Option<PathBuf>,
    pub program_data: Option<PathBuf>,
    pub user_library: Option<PathBuf>,
    pub library: Option<PathBuf>,
}

impl PresetBases {
    /// Bases from the home folder and (Windows) `%APPDATA%` / `%ProgramData%`.
    pub fn standard(os: Os, home: &str, app_data: &str, program_data: &str) -> Self {
        let some = |s: &str| (!s.is_empty()).then(|| PathBuf::from(s));
        let home = some(home);
        match os {
            Os::Macos => Self {
                documents: home.as_ref().map(|h| h.join("Documents")),
                user_library: home.as_ref().map(|h| h.join("Library")),
                library: Some(PathBuf::from("/Library")),
                ..Self::default()
            },
            Os::Windows => Self {
                documents: home.as_ref().map(|h| h.join("Documents")),
                app_data: some(app_data),
                program_data: some(program_data),
                ..Self::default()
            },
        }
    }

    fn get(&self, base: Base) -> Option<&Path> {
        match base {
            Base::Documents => self.documents.as_deref(),
            Base::AppData => self.app_data.as_deref(),
            Base::ProgramData => self.program_data.as_deref(),
            Base::UserLibrary => self.user_library.as_deref(),
            Base::Library => self.library.as_deref(),
        }
    }

    fn join(&self, (base, rel): (Base, &str)) -> Option<PathBuf> {
        let mut p = self.get(base)?.to_path_buf();
        p.extend(rel.split('/'));
        Some(p)
    }
}

/// A preset folder found on this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresetFolder {
    /// Logical root, e.g. `PRESETS_FABFILTER`.
    pub root: String,
    pub path: PathBuf,
    pub product: Product,
    pub portable: bool,
}

/// Every known preset folder that exists on this machine.
pub fn find_preset_folders(os: Os, bases: &PresetBases) -> Vec<PresetFolder> {
    let mut out = Vec::new();
    let mut claimed = Vec::new();
    for loc in LOCATIONS {
        let rel = match os {
            Os::Macos => loc.macos,
            Os::Windows => loc.windows,
        };
        let Some(path) = rel.and_then(|r| bases.join(r)) else {
            continue;
        };
        claimed.push(path.clone());
        if path.is_dir() {
            out.push(PresetFolder {
                root: root_name(loc.id),
                path,
                product: Product {
                    canonical_id: ProductId(format!("{PRODUCT_PREFIX}{}", loc.id)),
                    vendor: loc.vendor.into(),
                    name: loc.name.into(),
                },
                portable: loc.portable,
            });
        }
    }
    if let Some(standard) = bases.join(standard_folder(os))
        && let Ok(entries) = std::fs::read_dir(&standard)
    {
        let mut subdirs: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir() && !claimed.contains(p) && !is_hidden(p))
            .collect();
        subdirs.sort();
        for path in subdirs {
            let Some(vendor) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else {
                continue;
            };
            let id = format!("standard-{}", slug(&vendor));
            out.push(PresetFolder {
                root: root_name(&id),
                product: Product {
                    canonical_id: ProductId(format!("{PRODUCT_PREFIX}{id}")),
                    name: format!("{vendor} presets"),
                    vendor,
                },
                path,
                portable: false,
            });
        }
    }
    out
}

/// One item (and product) per folder, with a content hash of its files.
/// Empty folders are left out.
pub fn preset_items(folders: &[PresetFolder], os: Os) -> (Vec<Product>, Vec<Item>) {
    let mut products = Vec::new();
    let mut items = Vec::new();
    for f in folders {
        let Some(summary) = summarize(&f.path) else {
            continue;
        };
        products.push(f.product.clone());
        items.push(Item {
            kind: ItemKind::Folder,
            product_id: Some(f.product.canonical_id.clone()),
            format: None,
            platform: os,
            arch: None,
            version: None,
            path: PortablePath::logical(f.root.clone(), ""),
            content_hash: Some(
                format!("blake3:{}", summary.hash)
                    .try_into()
                    .expect("valid hash"),
            ),
            size: Some(summary.size),
            mtime: summary.mtime,
            platform_portable: f.portable,
            confidence: Confidence::Medium,
            requires: Vec::new(),
            needs_files: Vec::new(),
        });
    }
    (products, items)
}

/// Where a preset folder lives on `os`, for people: `Documents\Kilohearts`,
/// `/Library/Arturia/Presets`. `None` for anything that isn't one.
pub fn location(product: &Product, os: Os) -> Option<String> {
    let id = product.canonical_id.0.strip_prefix(PRODUCT_PREFIX)?;
    if let Some(loc) = LOCATIONS.iter().find(|l| l.id == id) {
        let at = match os {
            Os::Macos => loc.macos,
            Os::Windows => loc.windows,
        }?;
        return Some(show(at, os));
    }
    id.starts_with("standard-").then(|| {
        format!(
            "{}{}{}",
            show(standard_folder(os), os),
            separator(os),
            product.vendor
        )
    })
}

fn show((base, rel): (Base, &str), os: Os) -> String {
    let base = match base {
        Base::Documents if os == Os::Macos => "~/Documents",
        Base::Documents => "Documents",
        Base::AppData => "%APPDATA%",
        Base::ProgramData => "C:\\ProgramData",
        Base::UserLibrary => "~/Library",
        Base::Library => "/Library",
    };
    let sep = separator(os);
    format!("{base}{sep}{}", rel.replace('/', sep))
}

fn separator(os: Os) -> &'static str {
    match os {
        Os::Macos => "/",
        Os::Windows => "\\",
    }
}

pub fn is_preset_item(item: &Item) -> bool {
    item.product_id
        .as_ref()
        .is_some_and(|id| id.0.starts_with(PRODUCT_PREFIX))
}

struct Summary {
    hash: String,
    size: u64,
    mtime: Option<i64>,
}

/// Hash of the folder's files: relative path, then content (or size, for big
/// files and big folders). `None` for a folder without files.
fn summarize(dir: &Path) -> Option<Summary> {
    let mut files: Vec<(String, PathBuf, std::fs::Metadata)> = walkdir::WalkDir::new(dir)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| e.depth() == 0 || !is_hidden(e.path()))
        .flatten()
        .filter(|e| e.file_type().is_file() && !is_junk(e.path()))
        .take(MAX_FILES)
        .filter_map(|e| {
            let rel = e.path().strip_prefix(dir).ok()?;
            let rel: Vec<String> = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect();
            let meta = e.metadata().ok()?;
            Some((rel.join("/"), e.into_path(), meta))
        })
        .collect();
    if files.is_empty() {
        return None;
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    let hash_contents = files.len() <= MAX_HASHED_FILES;
    let mut h = blake3::Hasher::new();
    let mut size = 0;
    let mut mtime = None;
    for (rel, path, meta) in &files {
        size += meta.len();
        let modified = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .and_then(|d| i64::try_from(d.as_secs()).ok());
        mtime = mtime.max(modified);
        h.update(rel.as_bytes());
        h.update(b"\0");
        let content = (hash_contents && meta.len() <= HASH_LIMIT)
            .then(|| std::fs::read(path).ok())
            .flatten();
        match content {
            Some(bytes) => {
                h.update(b"c");
                h.update(blake3::hash(&bytes).as_bytes());
            }
            None => {
                h.update(b"s");
                h.update(&meta.len().to_le_bytes());
            }
        }
    }
    Some(Summary {
        hash: h.finalize().to_hex().to_string(),
        size,
        mtime,
    })
}

fn is_hidden(p: &Path) -> bool {
    p.file_name()
        .is_some_and(|n| n.to_string_lossy().starts_with('.'))
}

/// OS litter that differs between machines and means nothing.
fn is_junk(p: &Path) -> bool {
    let name = p
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    name.starts_with('.') || matches!(name.as_str(), "thumbs.db" | "desktop.ini")
}

fn root_name(id: &str) -> String {
    format!("PRESETS_{}", id.to_ascii_uppercase().replace('-', "_"))
}

fn slug(s: &str) -> String {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join("-")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, content: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn mac_bases(home: &Path) -> PresetBases {
        PresetBases {
            documents: Some(home.join("Documents")),
            user_library: Some(home.join("Library")),
            library: Some(home.join("SharedLibrary")),
            ..PresetBases::default()
        }
    }

    #[test]
    fn ids_and_roots_are_unique() {
        let mut ids: Vec<&str> = LOCATIONS.iter().map(|l| l.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), LOCATIONS.len());
        assert!(
            LOCATIONS
                .iter()
                .all(|l| l.macos.is_some() || l.windows.is_some())
        );
    }

    #[test]
    fn finds_vendor_and_standard_folders() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        write(
            &home.join("Documents/FabFilter/Presets/Pro-Q 4/Air.ffp"),
            "air",
        );
        write(
            &home.join("Library/Audio/Presets/u-he/Diva/Bass.h2p"),
            "bass",
        );
        write(
            &home.join("Library/Audio/Presets/Acme/Verb/Hall.aupreset"),
            "hall",
        );
        write(&home.join("Library/Audio/Presets/.hidden/x"), "x");
        std::fs::create_dir_all(home.join("Documents/Vital")).unwrap();

        let folders = find_preset_folders(Os::Macos, &mac_bases(home));
        let ids: Vec<&str> = folders
            .iter()
            .map(|f| f.product.canonical_id.0.as_str())
            .collect();
        // u-he is claimed by its own entry, not listed twice.
        assert_eq!(
            ids,
            [
                "presets/fabfilter",
                "presets/u-he",
                "presets/vital",
                "presets/standard-acme"
            ]
        );

        let (products, items) = preset_items(&folders, Os::Macos);
        // The empty Vital folder is left out.
        assert_eq!(products.len(), 3);
        let fab = &items[0];
        assert_eq!(fab.kind, ItemKind::Folder);
        assert_eq!(fab.path, PortablePath::logical("PRESETS_FABFILTER", ""));
        assert!(fab.platform_portable);
        assert_eq!(fab.size, Some(3));
        assert!(!items[2].platform_portable);
    }

    #[test]
    fn hash_follows_files_not_machines() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        for dir in [a.path(), b.path()] {
            write(&dir.join("Bank/One.fxp"), "one");
            write(&dir.join("Two.fxp"), "two");
        }
        write(&b.path().join(".DS_Store"), "junk");
        write(&b.path().join("Thumbs.db"), "junk");
        let ha = summarize(a.path()).unwrap().hash;
        assert_eq!(ha, summarize(b.path()).unwrap().hash);
        write(&b.path().join("Two.fxp"), "TWO");
        assert_ne!(ha, summarize(b.path()).unwrap().hash);
    }

    #[test]
    fn locations_read_like_the_os() {
        let product = |id: &str, vendor: &str| Product {
            canonical_id: ProductId(format!("{PRODUCT_PREFIX}{id}")),
            vendor: vendor.into(),
            name: String::new(),
        };
        let arturia = product("arturia", "Arturia");
        assert_eq!(
            location(&arturia, Os::Windows).as_deref(),
            Some("C:\\ProgramData\\Arturia\\Presets")
        );
        assert_eq!(
            location(&arturia, Os::Macos).as_deref(),
            Some("/Library/Arturia/Presets")
        );
        assert_eq!(
            location(&product("standard-valhalla", "Valhalla"), Os::Windows).as_deref(),
            Some("Documents\\VST3 Presets\\Valhalla")
        );
        assert_eq!(
            location(&product("standard-valhalla", "Valhalla"), Os::Macos).as_deref(),
            Some("~/Library/Audio/Presets/Valhalla")
        );
        let plugin = Product {
            canonical_id: ProductId("acme-verb".into()),
            ..arturia
        };
        assert_eq!(location(&plugin, Os::Macos), None);
    }
}
