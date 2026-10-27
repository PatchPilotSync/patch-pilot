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
//!
//! Each file in a folder is also an item of its own (no product, so the
//! comparison still shows one row per folder), with its content hash, so
//! Bring over can carry presets to a linked machine (decision #63).

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
/// Preset files up to this size get a content hash of their own. Bigger ones
/// can't travel over the link (about 2 MB per file), so only their size is
/// recorded.
const FILE_HASH_LIMIT: u64 = 2_000_000;
/// Never list more preset files than this across all folders, so the
/// manifest stays small enough for the relay. Folders past
/// [`MAX_HASHED_FILES`] aren't listed file by file either.
const MAX_LISTED_FILES: usize = 10_000;
/// Logical roots of preset folders start with this.
pub const ROOT_PREFIX: &str = "PRESETS_";
/// Compiled code is never listed or written, wherever it sits (CLAUDE.md rule 2).
const NEVER_SHARED_EXT: [&str; 8] = ["dll", "dylib", "so", "exe", "app", "msi", "pkg", "dmg"];

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
    /// Every user's documents: macOS `/Users/Shared`, Windows
    /// `C:\Users\Public\Documents`.
    Shared,
}

/// Places a folder can be on one OS, most likely first. The first that
/// exists is the one used.
pub type Places = &'static [(Base, &'static str)];

#[derive(Debug, Clone, Copy)]
pub struct PresetLocation {
    /// Slug, unique, used in the product ID and the logical root.
    pub id: &'static str,
    pub vendor: &'static str,
    /// What the folder holds, e.g. "Serum presets".
    pub name: &'static str,
    pub macos: Places,
    pub windows: Places,
    /// True when both OSes lay the folder out the same way, so a Mac and a PC
    /// should hold the same files.
    pub portable: bool,
}

impl PresetLocation {
    fn places(&self, os: Os) -> Places {
        match os {
            Os::Macos => self.macos,
            Os::Windows => self.windows,
        }
    }
}

/// A folder at the same place under Documents on both OSes.
macro_rules! both {
    ($id:literal, $vendor:literal, $name:literal, $rel:literal) => {
        PresetLocation {
            id: $id,
            vendor: $vendor,
            name: $name,
            macos: &[(Base::Documents, $rel)],
            windows: &[(Base::Documents, $rel)],
            portable: true,
        }
    };
}

/// Vendor preset folders. Keep sorted by vendor.
pub const LOCATIONS: &[PresetLocation] = &[
    PresetLocation {
        id: "arturia",
        vendor: "Arturia",
        name: "Arturia presets",
        macos: &[(Base::Library, "Arturia/Presets")],
        windows: &[(Base::ProgramData, "Arturia/Presets")],
        portable: true,
    },
    both!(
        "fabfilter",
        "FabFilter",
        "FabFilter presets",
        "FabFilter/Presets"
    ),
    both!(
        "ik-multimedia",
        "IK Multimedia",
        "IK Multimedia presets",
        "IK Multimedia"
    ),
    both!("izotope", "iZotope", "iZotope presets", "iZotope"),
    both!(
        "kilohearts",
        "Kilohearts",
        "Kilohearts presets",
        "Kilohearts"
    ),
    both!(
        "native-instruments",
        "Native Instruments",
        "Native Instruments User Content",
        "Native Instruments/User Content"
    ),
    PresetLocation {
        id: "soundtoys",
        vendor: "Soundtoys",
        name: "Soundtoys presets",
        macos: &[(Base::Shared, "Soundtoys/Soundtoys 5")],
        windows: &[(Base::Shared, "Soundtoys/Soundtoys 5")],
        portable: true,
    },
    PresetLocation {
        id: "spectrasonics",
        vendor: "Spectrasonics",
        name: "Omnisphere user patches",
        // STEAM is in /Library unless moved; ~/Library has a link to it.
        macos: &[
            (
                Base::Library,
                "Application Support/Spectrasonics/STEAM/Omnisphere/Settings Library/Patches/User",
            ),
            (
                Base::UserLibrary,
                "Application Support/Spectrasonics/STEAM/Omnisphere/Settings Library/Patches/User",
            ),
        ],
        windows: &[(
            Base::ProgramData,
            "Spectrasonics/STEAM/Omnisphere/Settings Library/Patches/User",
        )],
        portable: true,
    },
    both!(
        "surge-xt",
        "Surge Synth Team",
        "Surge XT user data",
        "Surge XT"
    ),
    PresetLocation {
        id: "tal",
        vendor: "TAL Software",
        name: "TAL presets",
        macos: &[(Base::UserLibrary, "Application Support/ToguAudioLine")],
        windows: &[(Base::AppData, "ToguAudioLine")],
        portable: true,
    },
    PresetLocation {
        id: "u-he",
        vendor: "u-he",
        name: "u-he presets",
        macos: &[
            (Base::UserLibrary, "Audio/Presets/u-he"),
            (Base::Library, "Audio/Presets/u-he"),
        ],
        windows: &[(Base::Documents, "u-he")],
        // Windows keeps them in `<Product>.data/Presets`, macOS in `<Product>`.
        portable: false,
    },
    PresetLocation {
        id: "valhalla-dsp",
        vendor: "Valhalla DSP",
        name: "Valhalla presets",
        macos: &[(Base::UserLibrary, "Application Support/Valhalla DSP, LLC")],
        windows: &[(Base::AppData, "Valhalla DSP, LLC")],
        portable: true,
    },
    PresetLocation {
        id: "vital",
        vendor: "Vital Audio",
        name: "Vital presets",
        macos: &[
            (Base::Documents, "Vital"),
            (Base::Library, "Audio/Presets/Vital"),
            (Base::UserLibrary, "Audio/Presets/Vital"),
        ],
        windows: &[(Base::Documents, "Vital")],
        portable: true,
    },
    // Serum's installer puts them in /Library on a Mac (or ~/Library when
    // installed for one user), and in Documents on Windows.
    PresetLocation {
        id: "serum",
        vendor: "Xfer Records",
        name: "Serum presets",
        macos: &[
            (Base::Library, "Audio/Presets/Xfer Records/Serum Presets"),
            (
                Base::UserLibrary,
                "Audio/Presets/Xfer Records/Serum Presets",
            ),
            (Base::Documents, "Xfer/Serum Presets"),
        ],
        windows: &[(Base::Documents, "Xfer/Serum Presets")],
        portable: true,
    },
    PresetLocation {
        id: "serum-2",
        vendor: "Xfer Records",
        name: "Serum 2 presets",
        macos: &[
            (Base::Library, "Audio/Presets/Xfer Records/Serum 2 Presets"),
            (
                Base::UserLibrary,
                "Audio/Presets/Xfer Records/Serum 2 Presets",
            ),
            (Base::Documents, "Xfer/Serum 2 Presets"),
        ],
        windows: &[(Base::Documents, "Xfer/Serum 2 Presets")],
        portable: true,
    },
];

/// A standard preset folder whose vendor subfolders each become an item.
struct Standard {
    at: (Base, &'static str),
    /// Start of each subfolder's ID, before the vendor's slug.
    prefix: &'static str,
    /// Said after the name when an earlier standard folder has the vendor too.
    again: &'static str,
    /// The user's own folder. The others skip DAW makers' and the OS's
    /// presets, which come with them.
    own: bool,
}

/// The standard preset folders on `os`, the user's own first.
fn standard_folders(os: Os) -> &'static [Standard] {
    match os {
        Os::Macos => &[
            Standard {
                at: (Base::UserLibrary, "Audio/Presets"),
                prefix: "standard-",
                again: "",
                own: true,
            },
            Standard {
                at: (Base::Library, "Audio/Presets"),
                prefix: "standard-all-",
                again: " for all users",
                own: false,
            },
        ],
        Os::Windows => &[
            Standard {
                at: (Base::Documents, "VST3 Presets"),
                prefix: "standard-",
                again: "",
                own: true,
            },
            Standard {
                at: (Base::AppData, "VST3 Presets"),
                prefix: "standard-roaming-",
                again: " in AppData",
                own: false,
            },
            Standard {
                at: (Base::ProgramData, "VST3 Presets"),
                prefix: "standard-all-",
                again: " for all users",
                own: false,
            },
        ],
    }
}

/// The standard folder an ID from [`standard_folders`] belongs to. The
/// longest prefix wins, so `standard-all-x` isn't read as vendor `all-x` of
/// the user's folder.
fn standard_for(id: &str, os: Os) -> Option<&'static Standard> {
    standard_folders(os)
        .iter()
        .filter(|s| id.starts_with(s.prefix))
        .max_by_key(|s| s.prefix.len())
}

/// Where vendors keep their own folders, searched for preset folders by name
/// (see [`find_preset_folders`]).
fn vendor_places(os: Os) -> &'static [(Base, &'static str)] {
    match os {
        Os::Macos => &[
            (Base::Documents, ""),
            (Base::UserLibrary, "Application Support"),
            (Base::Library, "Application Support"),
            (Base::Shared, ""),
        ],
        Os::Windows => &[
            (Base::Documents, ""),
            (Base::AppData, ""),
            (Base::ProgramData, ""),
            (Base::Shared, ""),
        ],
    }
}

/// Catalog vendors whose folders hold a DAW's own files, which the DAW
/// scanners handle, or the OS's.
const NOT_PRESET_VENDORS: [&str; 6] = [
    "ableton",
    "apple",
    "avid",
    "cockos",
    "image-line",
    "steinberg",
];

/// Product IDs of preset folders found by name start with this, after
/// [`PRODUCT_PREFIX`].
const FOUND_PREFIX: &str = "found-";

/// This machine's base folders. `None` where unknown or not on this OS.
#[derive(Debug, Clone, Default)]
pub struct PresetBases {
    pub documents: Option<PathBuf>,
    pub app_data: Option<PathBuf>,
    pub program_data: Option<PathBuf>,
    pub user_library: Option<PathBuf>,
    pub library: Option<PathBuf>,
    pub shared: Option<PathBuf>,
}

impl PresetBases {
    /// Bases from the home folder and (Windows) `%APPDATA%`, `%ProgramData%`
    /// and `%PUBLIC%`.
    pub fn standard(os: Os, home: &str, app_data: &str, program_data: &str, public: &str) -> Self {
        let some = |s: &str| (!s.is_empty()).then(|| PathBuf::from(s));
        let home = some(home);
        match os {
            Os::Macos => Self {
                documents: home.as_ref().map(|h| h.join("Documents")),
                user_library: home.as_ref().map(|h| h.join("Library")),
                library: Some(PathBuf::from("/Library")),
                shared: Some(PathBuf::from("/Users/Shared")),
                ..Self::default()
            },
            Os::Windows => Self {
                documents: home.as_ref().map(|h| h.join("Documents")),
                app_data: some(app_data),
                program_data: some(program_data),
                shared: some(public).map(|p| p.join("Documents")),
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
            Base::Shared => self.shared.as_deref(),
        }
    }

    fn join(&self, (base, rel): (Base, &str)) -> Option<PathBuf> {
        let mut p = self.get(base)?.to_path_buf();
        p.extend(rel.split('/').filter(|s| !s.is_empty()));
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

/// Every preset folder on this machine, in three passes:
/// - the vendor folders in [`LOCATIONS`], at the first of their places that
///   exists;
/// - each vendor subfolder of the standard preset folders (AU and VST3);
/// - folders named like presets ("Presets", "User Presets", "Serum 2
///   Presets") up to two levels inside a folder named after a vendor the
///   catalog knows, in Documents, Application Support (macOS), `%APPDATA%`
///   and `%ProgramData%` (Windows) and the shared documents folder:
///   `ProgramData\Valhalla DSP, LLC\ValhallaVintageVerb\Presets`.
///
/// A folder inside or around one already listed is left out, so no file
/// counts twice.
pub fn find_preset_folders(os: Os, bases: &PresetBases) -> Vec<PresetFolder> {
    let mut out = Vec::new();
    // Every place a known folder may be, found or not, so the later passes
    // never list it again under another name.
    let mut claimed: Vec<PathBuf> = Vec::new();
    for loc in LOCATIONS {
        let places: Vec<PathBuf> = loc
            .places(os)
            .iter()
            .filter_map(|&at| bases.join(at))
            .collect();
        if let Some(path) = places.iter().find(|p| p.is_dir()) {
            out.push(PresetFolder {
                root: root_name(loc.id),
                path: path.clone(),
                product: Product {
                    canonical_id: ProductId(format!("{PRODUCT_PREFIX}{}", loc.id)),
                    vendor: loc.vendor.into(),
                    name: loc.name.into(),
                },
                portable: loc.portable,
            });
        }
        claimed.extend(places);
    }
    let overlaps = |p: &Path, claimed: &[PathBuf]| {
        claimed.iter().any(|c| c.starts_with(p) || p.starts_with(c))
    };
    let mut standard_vendors = Vec::new();
    for standard in standard_folders(os) {
        let Some(dir) = bases.join(standard.at) else {
            continue;
        };
        for path in subdirs(&dir) {
            if overlaps(&path, &claimed) {
                continue;
            }
            let vendor = file_name(&path);
            if !standard.own && is_daw_vendor(&vendor) {
                continue;
            }
            // The same vendor in the user's and the shared folder: say which.
            let again = if standard_vendors.contains(&vendor) {
                standard.again
            } else {
                ""
            };
            let name = format!("{vendor} presets{again}");
            let id = format!("{}{}", standard.prefix, slug(&vendor));
            standard_vendors.push(vendor.clone());
            out.push(PresetFolder {
                root: root_name(&id),
                product: Product {
                    canonical_id: ProductId(format!("{PRODUCT_PREFIX}{id}")),
                    name,
                    vendor,
                },
                path,
                portable: false,
            });
        }
        claimed.push(dir);
    }
    let catalog = patchpilot_catalog::current();
    for &at in vendor_places(os) {
        let Some(dir) = bases.join(at) else {
            continue;
        };
        for vendor_dir in subdirs(&dir) {
            let name = file_name(&vendor_dir);
            let Some(vendor) = catalog
                .vendor_named(&name)
                .filter(|_| !is_daw_vendor(&name))
            else {
                continue;
            };
            if claimed.iter().any(|c| vendor_dir.starts_with(c)) {
                continue;
            }
            for path in named_preset_dirs(&vendor_dir) {
                if overlaps(&path, &claimed) {
                    continue;
                }
                let Ok(rel) = path.strip_prefix(&vendor_dir) else {
                    continue;
                };
                let rel: Vec<String> = rel
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect();
                let id = format!("{FOUND_PREFIX}{}-{}", vendor.id, slug(&rel.join(" ")));
                // The same folder in a user's and a shared place: the user's.
                let product_id = format!("{PRODUCT_PREFIX}{id}");
                if out.iter().any(|f| f.product.canonical_id.0 == product_id) {
                    continue;
                }
                let parent = match rel.len() {
                    1 => vendor.name.clone(),
                    _ => rel[rel.len() - 2].clone(),
                };
                out.push(PresetFolder {
                    root: root_name(&id),
                    product: Product {
                        canonical_id: ProductId(product_id),
                        vendor: vendor.name.clone(),
                        name: found_name(&parent, &rel[rel.len() - 1]),
                    },
                    path: path.clone(),
                    portable: false,
                });
                claimed.push(path);
            }
        }
    }
    out
}

fn is_daw_vendor(name: &str) -> bool {
    patchpilot_catalog::current()
        .vendor_named(name)
        .is_some_and(|v| NOT_PRESET_VENDORS.contains(&v.id.as_str()))
}

/// Visible subfolders of `dir`, sorted.
fn subdirs(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir() && !is_hidden(p))
        .collect();
    dirs.sort();
    dirs
}

fn file_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Folders one or two levels inside `vendor_dir` whose name has the word
/// "presets", outermost only.
fn named_preset_dirs(vendor_dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for child in subdirs(vendor_dir) {
        if is_preset_name(&file_name(&child)) {
            out.push(child);
            continue;
        }
        out.extend(
            subdirs(&child)
                .into_iter()
                .filter(|p| is_preset_name(&file_name(p))),
        );
    }
    out
}

fn is_preset_name(name: &str) -> bool {
    slug(name).split('-').any(|w| w == "presets")
}

/// "Serum 2 Presets" -> "Serum 2 presets"; "Presets" in "Elevate" ->
/// "Elevate presets"; "USER Presets" in "Fresh Air" -> "Fresh Air user
/// presets".
fn found_name(parent: &str, name: &str) -> String {
    const PLAIN: [&str; 6] = ["presets", "user", "factory", "my", "custom", "own"];
    let words: Vec<&str> = name.split_whitespace().collect();
    let plain = words
        .iter()
        .all(|w| PLAIN.contains(&w.to_ascii_lowercase().as_str()));
    let words: Vec<String> = words
        .iter()
        .map(|w| {
            if PLAIN.contains(&w.to_ascii_lowercase().as_str()) {
                w.to_ascii_lowercase()
            } else {
                (*w).to_owned()
            }
        })
        .collect();
    let name = words.join(" ");
    if plain {
        format!("{parent} {name}")
    } else {
        name
    }
}

/// One item (and product) per folder, with a content hash of its files,
/// followed by one product-less item per file in it (see [`is_preset_file`]).
/// Empty folders are left out.
pub fn preset_items(folders: &[PresetFolder], os: Os) -> (Vec<Product>, Vec<Item>) {
    let mut products = Vec::new();
    let mut items = Vec::new();
    let mut listed = 0;
    for f in folders {
        let Some(summary) = summarize(&f.path) else {
            continue;
        };
        products.push(f.product.clone());
        let files = if listed + summary.files.len() <= MAX_LISTED_FILES {
            listed += summary.files.len();
            summary.files
        } else {
            Vec::new()
        };
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
        let standard = f.root.starts_with(&root_name("standard-"));
        items.extend(files.into_iter().map(|file| {
            Item {
                kind: ItemKind::DawAsset,
                product_id: None,
                format: None,
                platform: os,
                arch: None,
                version: None,
                // A VST3 preset in the standard folder sits in the same place on
                // both OSes; the AU presets beside it on a Mac are Mac-only.
                platform_portable: f.portable || (standard && is_vst3_preset(&file.rel)),
                path: PortablePath::logical(f.root.clone(), file.rel),
                content_hash: file
                    .hash
                    .map(|h| format!("blake3:{h}").try_into().expect("valid hash")),
                size: Some(file.size),
                mtime: file.mtime,
                confidence: Confidence::High,
                requires: Vec::new(),
                needs_files: Vec::new(),
            }
        }));
    }
    (products, items)
}

/// One file in a preset folder: an item at `PRESETS_<VENDOR>/<rel>` with no
/// product. The comparison counts the folder, not its files; Bring over
/// carries the files.
pub fn is_preset_file(item: &Item) -> bool {
    item.product_id.is_none()
        && matches!(&item.path, PortablePath::Logical { root, rel }
            if is_preset_root(root) && !rel.is_empty())
}

pub fn is_preset_root(root: &str) -> bool {
    root.starts_with(ROOT_PREFIX)
}

/// True for a `/`-separated path below a preset folder that may be listed
/// and written: no parent or hidden parts, no compiled code. Checked before
/// writing a file that came from another machine.
pub fn is_shareable_file(rel: &str) -> bool {
    let parts: Vec<&str> = rel.split('/').collect();
    let safe = parts
        .iter()
        .all(|p| !p.is_empty() && !p.starts_with('.') && !p.contains(['\\', ':']));
    let name = parts[parts.len() - 1].to_ascii_lowercase();
    let ext = Path::new(&name)
        .extension()
        .map(|e| e.to_string_lossy().into_owned());
    safe && !matches!(name.as_str(), "thumbs.db" | "desktop.ini")
        && !ext.is_some_and(|e| NEVER_SHARED_EXT.contains(&e.as_str()))
}

fn is_vst3_preset(rel: &str) -> bool {
    rel.to_ascii_lowercase().ends_with(".vstpreset")
}

/// True when `root` is a vendor folder from [`LOCATIONS`] with a place on `os`.
pub fn is_vendor_root(root: &str, os: Os) -> bool {
    LOCATIONS
        .iter()
        .any(|l| root_name(l.id) == root && !l.places(os).is_empty())
}

/// Where preset folder `root` belongs on this machine when its scan didn't
/// find it: the vendor's place that exists, else its first; for a standard
/// folder, a subfolder there whose name matches. `None` when it can't be told,
/// as for folders found by name.
pub fn default_dir(root: &str, os: Os, bases: &PresetBases) -> Option<PathBuf> {
    if let Some(loc) = LOCATIONS.iter().find(|l| root_name(l.id) == root) {
        let places: Vec<PathBuf> = loc
            .places(os)
            .iter()
            .filter_map(|&at| bases.join(at))
            .collect();
        return places
            .iter()
            .find(|p| p.is_dir())
            .or(places.first())
            .cloned();
    }
    standard_folders(os).iter().find_map(|s| {
        subdirs(&bases.join(s.at)?)
            .into_iter()
            .find(|p| root_name(&format!("{}{}", s.prefix, slug(&file_name(p)))) == root)
    })
}

/// Where a preset folder lives on `os`, for people: `Documents\Kilohearts`,
/// `/Library/Arturia/Presets`. `None` for anything that isn't one.
pub fn location(product: &Product, os: Os) -> Option<String> {
    let id = product.canonical_id.0.strip_prefix(PRODUCT_PREFIX)?;
    if let Some(loc) = LOCATIONS.iter().find(|l| l.id == id) {
        return loc.places(os).first().map(|&at| show(at, os));
    }
    let standard = standard_for(id, os)?;
    Some(format!(
        "{}{}{}",
        show(standard.at, os),
        separator(os),
        product.vendor
    ))
}

fn show((base, rel): (Base, &str), os: Os) -> String {
    let base = match base {
        Base::Documents if os == Os::Macos => "~/Documents",
        Base::Documents => "Documents",
        Base::AppData => "%APPDATA%",
        Base::ProgramData => "C:\\ProgramData",
        Base::UserLibrary => "~/Library",
        Base::Library => "/Library",
        Base::Shared if os == Os::Macos => "/Users/Shared",
        Base::Shared => "C:\\Users\\Public\\Documents",
    };
    let sep = separator(os);
    if rel.is_empty() {
        return base.to_owned();
    }
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
    /// Each file, for folders up to [`MAX_HASHED_FILES`] files.
    files: Vec<FileSummary>,
}

struct FileSummary {
    rel: String,
    size: u64,
    mtime: Option<i64>,
    /// BLAKE3 hex of the contents, up to [`FILE_HASH_LIMIT`].
    hash: Option<String>,
}

/// Hash of the folder's files: relative path, then content (or size, for big
/// files and big folders), and each file's own hash. `None` for a folder
/// without files.
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
    let mut listed = Vec::new();
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
        let file_hash = (hash_contents && meta.len() <= FILE_HASH_LIMIT)
            .then(|| std::fs::read(path).ok())
            .flatten()
            .map(|bytes| blake3::hash(&bytes));
        match file_hash.filter(|_| meta.len() <= HASH_LIMIT) {
            Some(hash) => {
                h.update(b"c");
                h.update(hash.as_bytes());
            }
            None => {
                h.update(b"s");
                h.update(&meta.len().to_le_bytes());
            }
        }
        if hash_contents && is_shareable_file(rel) {
            listed.push(FileSummary {
                rel: rel.clone(),
                size: meta.len(),
                mtime: modified,
                hash: file_hash.map(|h| h.to_hex().to_string()),
            });
        }
    }
    Some(Summary {
        hash: h.finalize().to_hex().to_string(),
        size,
        mtime,
        files: listed,
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
    format!("{ROOT_PREFIX}{}", id.to_ascii_uppercase().replace('-', "_"))
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
    use patchpilot_core::ContentHash;

    fn write(path: &Path, content: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn mac_bases(home: &Path) -> PresetBases {
        PresetBases {
            documents: Some(home.join("Documents")),
            user_library: Some(home.join("Library")),
            library: Some(home.join("SharedLibrary")),
            shared: Some(home.join("Shared")),
            ..PresetBases::default()
        }
    }

    fn windows_bases(home: &Path) -> PresetBases {
        PresetBases {
            documents: Some(home.join("Documents")),
            app_data: Some(home.join("AppData")),
            program_data: Some(home.join("ProgramData")),
            shared: Some(home.join("Public")),
            ..PresetBases::default()
        }
    }

    fn found(folders: &[PresetFolder]) -> Vec<(&str, &str, &str)> {
        folders
            .iter()
            .map(|f| {
                (
                    f.product.canonical_id.0.as_str(),
                    f.product.vendor.as_str(),
                    f.product.name.as_str(),
                )
            })
            .collect()
    }

    #[test]
    fn serum_on_a_mac_is_in_the_shared_library() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        let shared = home.join("SharedLibrary/Audio/Presets");
        write(
            &shared.join("Xfer Records/Serum 2 Presets/Presets/User/Bass.SerumPreset"),
            "bass",
        );
        write(&shared.join("Acme/Verb/Hall.aupreset"), "hall");
        write(&shared.join("Apple/AUDelay/Echo.aupreset"), "echo");
        write(
            &home.join("Library/Audio/Presets/Acme/Verb/Room.aupreset"),
            "room",
        );
        let bases = mac_bases(home);
        let folders = find_preset_folders(Os::Macos, &bases);
        // Xfer Records isn't listed again around Serum 2's folder.
        assert_eq!(
            found(&folders),
            [
                ("presets/serum-2", "Xfer Records", "Serum 2 presets"),
                ("presets/standard-acme", "Acme", "Acme presets"),
                (
                    "presets/standard-all-acme",
                    "Acme",
                    "Acme presets for all users"
                ),
            ]
        );
        assert_eq!(folders[0].path, shared.join("Xfer Records/Serum 2 Presets"));
        assert_eq!(folders[0].root, "PRESETS_SERUM_2");
        assert_eq!(
            default_dir("PRESETS_SERUM_2", Os::Macos, &bases),
            Some(shared.join("Xfer Records/Serum 2 Presets"))
        );
        assert_eq!(
            default_dir("PRESETS_STANDARD_ALL_ACME", Os::Macos, &bases),
            Some(shared.join("Acme"))
        );
        // Serum's own folder on Windows is still the same folder.
        write(
            &home.join("Documents/Xfer/Serum 2 Presets/Presets/User/Bass.SerumPreset"),
            "bass",
        );
        let pc = find_preset_folders(Os::Windows, &windows_bases(home));
        assert_eq!(pc[0].root, "PRESETS_SERUM_2");
        assert!(pc[0].portable);
    }

    #[test]
    fn finds_preset_folders_in_vendor_folders_by_name() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        let data = home.join("ProgramData");
        write(
            &data.join("Valhalla DSP, LLC/ValhallaVintageVerb/Presets/Factory/Hall.vpreset"),
            "hall",
        );
        write(
            &data.join("Valhalla DSP, LLC/ValhallaVintageVerb/InstallerFiles/x"),
            "x",
        );
        write(
            &data.join("Slate Digital/Fresh Air/FACTORY Presets/Air.fxp"),
            "a",
        );
        write(
            &data.join("Slate Digital/Fresh Air/USER Presets/Mine.fxp"),
            "m",
        );
        write(&data.join("BABY Audio/presets/Warm.preset"), "w");
        write(
            &home.join("Documents/Universal Audio/Presets/Vocal.preset"),
            "v",
        );
        // A plug-in's own folder in the vendor's preset place is that folder.
        write(
            &home.join("AppData/Valhalla DSP, LLC/ValhallaRoom/User Presets/Big.vpreset"),
            "b",
        );
        // Not a vendor, a DAW's own folder, too deep, already listed.
        write(&home.join("Documents/Holiday/Presets/x.fxp"), "x");
        write(
            &home.join("Documents/Ableton/User Library/Presets/x.adv"),
            "x",
        );
        write(&data.join("Native Instruments/A/B/Presets/x.nksf"), "x");
        write(&home.join("Documents/Kilohearts/Presets/x.phaseplant"), "x");
        write(
            &home.join("Public/Soundtoys/Soundtoys 5/EchoBoy/Slap.preset"),
            "s",
        );

        let folders = find_preset_folders(Os::Windows, &windows_bases(home));
        assert_eq!(
            found(&folders),
            [
                ("presets/kilohearts", "Kilohearts", "Kilohearts presets"),
                ("presets/soundtoys", "Soundtoys", "Soundtoys presets"),
                ("presets/valhalla-dsp", "Valhalla DSP", "Valhalla presets"),
                (
                    "presets/found-universal-audio-presets",
                    "Universal Audio",
                    "Universal Audio presets"
                ),
                (
                    "presets/found-baby-audio-presets",
                    "Baby Audio",
                    "Baby Audio presets"
                ),
                (
                    "presets/found-slate-digital-fresh-air-factory-presets",
                    "Slate Digital",
                    "Fresh Air factory presets"
                ),
                (
                    "presets/found-slate-digital-fresh-air-user-presets",
                    "Slate Digital",
                    "Fresh Air user presets"
                ),
                (
                    "presets/found-valhalla-dsp-valhallavintageverb-presets",
                    "Valhalla DSP",
                    "ValhallaVintageVerb presets"
                ),
            ]
        );
        assert!(folders[3..].iter().all(|f| !f.portable));
        assert_eq!(
            folders[3].path,
            home.join("Documents/Universal Audio/Presets")
        );
        // Found by name: no fixed place to put it on another machine.
        assert_eq!(
            default_dir(&folders[3].root, Os::Windows, &windows_bases(home)),
            None
        );
        assert!(!is_vendor_root(&folders[3].root, Os::Windows));
        assert!(is_vendor_root("PRESETS_SOUNDTOYS", Os::Macos));
    }

    #[test]
    fn found_folders_are_named_for_people() {
        assert_eq!(
            found_name("Xfer Records", "Serum 2 Presets"),
            "Serum 2 presets"
        );
        assert_eq!(found_name("Elevate", "Presets"), "Elevate presets");
        assert_eq!(
            found_name("Fresh Air", "USER Presets"),
            "Fresh Air user presets"
        );
        assert_eq!(found_name("Baby Audio", "presets"), "Baby Audio presets");
        assert!(is_preset_name("FACTORY Presets"));
        assert!(is_preset_name("Serum_2_Presets"));
        assert!(!is_preset_name("PresetsBackup"));
        assert!(!is_preset_name("Settings"));
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
                .all(|l| !l.macos.is_empty() || !l.windows.is_empty())
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
        let folders: Vec<&Item> = items.iter().filter(|i| !is_preset_file(i)).collect();
        let fab = folders[0];
        assert_eq!(fab.kind, ItemKind::Folder);
        assert_eq!(fab.path, PortablePath::logical("PRESETS_FABFILTER", ""));
        assert!(fab.platform_portable);
        assert_eq!(fab.size, Some(3));
        assert!(!folders[2].platform_portable);

        // Each file follows its folder, with its own hash.
        let files: Vec<_> = items
            .iter()
            .filter(|i| is_preset_file(i))
            .map(|i| (&i.path, i.platform_portable))
            .collect();
        assert_eq!(
            files,
            [
                (
                    &PortablePath::logical("PRESETS_FABFILTER", "Pro-Q 4/Air.ffp"),
                    true
                ),
                (
                    &PortablePath::logical("PRESETS_U_HE", "Diva/Bass.h2p"),
                    false
                ),
                (
                    &PortablePath::logical("PRESETS_STANDARD_ACME", "Verb/Hall.aupreset"),
                    false
                ),
            ]
        );
        assert_eq!(
            items[1].content_hash.clone().unwrap(),
            ContentHash::try_from(format!("blake3:{}", blake3::hash(b"air").to_hex())).unwrap()
        );
    }

    #[test]
    fn big_files_get_no_hash_and_code_is_never_listed() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        let dir = home.join("Library/Audio/Presets/Acme");
        write(&dir.join("Verb/Hall.vstpreset"), "hall");
        write(&dir.join("Verb/Hall.aupreset"), "hall");
        write(&dir.join("Verb/helper.dylib"), "code");
        std::fs::write(
            dir.join("Verb/Big.vstpreset"),
            vec![0u8; FILE_HASH_LIMIT as usize + 1],
        )
        .unwrap();
        let folders = find_preset_folders(Os::Macos, &mac_bases(home));
        let (_, items) = preset_items(&folders, Os::Macos);
        let files: Vec<_> = items
            .iter()
            .filter(|i| is_preset_file(i))
            .map(|i| match &i.path {
                PortablePath::Logical { rel, .. } => {
                    (rel.as_str(), i.content_hash.is_some(), i.platform_portable)
                }
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(
            files,
            [
                ("Verb/Big.vstpreset", false, true),
                ("Verb/Hall.aupreset", true, false),
                ("Verb/Hall.vstpreset", true, true),
            ]
        );
    }

    #[test]
    fn shareable_files_stay_inside_the_folder() {
        assert!(is_shareable_file("Pro-Q 4/Air.ffp"));
        assert!(is_shareable_file("Bass.fxp"));
        for bad in [
            "../x.ffp",
            "a/../../x",
            ".hidden/x",
            "a/Thumbs.db",
            "a/b.dll",
            "a\\b",
            "C:/x",
            "",
        ] {
            assert!(!is_shareable_file(bad), "{bad}");
        }
    }

    #[test]
    fn default_dirs_follow_the_vendor_and_the_standard_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        let bases = mac_bases(home);
        assert_eq!(
            default_dir("PRESETS_FABFILTER", Os::Macos, &bases),
            Some(home.join("Documents").join("FabFilter").join("Presets"))
        );
        assert_eq!(
            default_dir("PRESETS_STANDARD_ACME", Os::Macos, &bases),
            None
        );
        std::fs::create_dir_all(home.join("Library/Audio/Presets/Acme")).unwrap();
        assert_eq!(
            default_dir("PRESETS_STANDARD_ACME", Os::Macos, &bases),
            Some(home.join("Library/Audio/Presets/Acme"))
        );
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
        assert_eq!(
            location(&product("standard-all-valhalla", "Valhalla"), Os::Macos).as_deref(),
            Some("/Library/Audio/Presets/Valhalla")
        );
        assert_eq!(
            location(&product("soundtoys", "Soundtoys"), Os::Windows).as_deref(),
            Some("C:\\Users\\Public\\Documents\\Soundtoys\\Soundtoys 5")
        );
        assert_eq!(
            location(&product("serum-2", "Xfer Records"), Os::Macos).as_deref(),
            Some("/Library/Audio/Presets/Xfer Records/Serum 2 Presets")
        );
        assert_eq!(
            location(&product("found-acme-presets", "Acme"), Os::Macos),
            None
        );
        let plugin = Product {
            canonical_id: ProductId("acme-verb".into()),
            ..arturia
        };
        assert_eq!(location(&plugin, Os::Macos), None);
    }
}
