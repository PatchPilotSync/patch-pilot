//! Ableton Live settings and User Library files, for parity between machines.
//!
//! What's read, and why (see decision #40):
//!
//! - **User Library** (`ABLETON_USER_LIBRARY`): templates (`Templates/*.als`,
//!   including `Default.als`), default device presets (`Defaults/`), racks and
//!   presets (`.adg`, `.adv`), Max for Live devices (`.amxd`), clips (`.alc`),
//!   grooves (`.agr`), and Collections tags: the `.xmp` files Live writes to
//!   `Ableton Folder Info/` (Live 12 uses its `12/` subfolder).
//! - **Preferences folder** (`ABLETON_PREFS`): `Options.txt` only.
//!
//! Never read or shared: `Preferences.cfg` (binary, and it mixes portable
//! settings with the audio device, MIDI ports, folder paths, plug-in folders
//! and the Ableton login), samples, Packs, project `Backup/` folders.
//! `Library.cfg` holds the Collection names but also machine paths (User
//! Library, Packs, Places), so it is only read to find the User Library.
//!
//! Live's preferences live in one folder per version: macOS
//! `~/Library/Preferences/Ableton/Live 12.1.5/`, Windows
//! `%APPDATA%\Ableton\Live 12.1.5\Preferences\`. The newest one wins.

use std::path::{Path, PathBuf};

use patchpilot_core::{
    Confidence, ContentHash, Item, ItemKind, Os, PortablePath, Product, ProductId,
};

use crate::version::{VersionOrder, compare_versions};

/// Logical root for the Ableton User Library in manifests.
pub const ABLETON_USER_LIBRARY: &str = "ABLETON_USER_LIBRARY";
/// Logical root for the newest Live version's preferences folder.
pub const ABLETON_PREFS: &str = "ABLETON_PREFS";
/// `Product::vendor` of every Ableton settings item; the app groups by it.
pub const VENDOR: &str = "Ableton Live";
/// Product IDs of Ableton settings items start with this.
pub const PRODUCT_PREFIX: &str = "ableton/";

/// Files in the preferences folder that are safe to share between machines.
const PORTABLE_PREFS: &[&str] = &["Options.txt"];

/// User Library folders never read: audio, project backups, caches.
const SKIPPED_DIRS: &[&str] = &["samples", "backup", "ableton project info", "recorded"];

/// Bigger files are skipped: they hold audio, not settings.
pub const MAX_FILE_BYTES: u64 = 20 * 1024 * 1024;

/// Where Ableton keeps one folder per Live version.
///
/// Windows: `%APPDATA%\Ableton`. macOS: `~/Library/Preferences/Ableton`.
pub fn prefs_base(os: Os, appdata: &str, home: &str) -> Option<PathBuf> {
    match os {
        Os::Windows if !appdata.is_empty() => Some(Path::new(appdata).join("Ableton")),
        Os::Macos if !home.is_empty() => Some(
            Path::new(home)
                .join("Library")
                .join("Preferences")
                .join("Ableton"),
        ),
        _ => None,
    }
}

/// The newest `Live x.y.z` preferences folder under `base`, if any.
///
/// Folder names look like `Live 12.1.5` or `Live 12.2b3 Beta`. Releases
/// beat betas, since most people try a beta beside the release they use. On Windows the files are one level
/// down, in `Preferences`.
pub fn latest_prefs_dir(base: &Path, os: Os) -> Option<PathBuf> {
    let mut best: Option<(String, bool, PathBuf)> = None;
    for entry in std::fs::read_dir(base).ok()?.filter_map(Result::ok) {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some((version, beta)) = parse_live_folder(&name) else {
            continue;
        };
        let dir = match os {
            Os::Windows => entry.path().join("Preferences"),
            Os::Macos => entry.path(),
        };
        if !dir.is_dir() {
            continue;
        }
        let better = best.as_ref().is_none_or(|(v, b, _)| {
            (*b && !beta) || (*b == beta && compare_versions(v, &version) == VersionOrder::Older)
        });
        if better {
            best = Some((version, beta, dir));
        }
    }
    best.map(|(_, _, dir)| dir)
}

/// `Live 12.1.5` → (`12.1.5`, false); `Live 12.2b3 Beta` → (`12.2b3`, true).
fn parse_live_folder(name: &str) -> Option<(String, bool)> {
    let rest = name.strip_prefix("Live ")?;
    let mut words = rest.split_whitespace();
    let version = words.next()?;
    if !version.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    let beta = words.any(|w| w.eq_ignore_ascii_case("beta"))
        || version.contains(|c: char| c.is_ascii_alphabetic());
    Some((version.to_owned(), beta))
}

/// The User Library folder `Library.cfg` points at, if it says.
///
/// Live writes it as a `<UserLibrary>` element holding a `<ProjectPath
/// Value="..."/>` (the folder) and usually a `<ProjectName
/// Value="User Library"/>`. Older versions store the parent in
/// `ProjectPath`, so the name is appended when the path doesn't end with it.
pub fn user_library_from_cfg(cfg: &str, os: Os) -> Option<PathBuf> {
    let start = cfg.find("<UserLibrary")?;
    let section = &cfg[start..];
    let section = section
        .find("</UserLibrary>")
        .map_or(section, |end| &section[..end]);
    let path = xml_value(section, "ProjectPath")?;
    if path.is_empty() {
        return None;
    }
    let name = xml_value(section, "ProjectName").filter(|n| !n.is_empty());
    let trimmed = path.trim_end_matches(['/', '\\']);
    let last = trimmed.rsplit(['/', '\\']).next().unwrap_or("");
    let mut dir = trimmed.to_owned();
    if let Some(name) = name
        && !last.eq_ignore_ascii_case(&name)
    {
        dir.push(os.separator());
        dir.push_str(&name);
    }
    Some(PathBuf::from(dir))
}

/// The `Value` attribute of the first `<tag ... Value="..."/>`, unescaped.
fn xml_value(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}");
    let mut from = 0;
    while let Some(at) = xml[from..].find(&open) {
        let start = from + at + open.len();
        from = start;
        // `<ProjectPathX` is another tag.
        if !xml[start..].starts_with(|c: char| c.is_whitespace() || c == '/' || c == '>') {
            continue;
        }
        let elem = &xml[start..start + xml[start..].find('>')?];
        let v = elem.find("Value=\"")? + "Value=\"".len();
        let end = elem[v..].find('"')?;
        return Some(unescape(&elem[v..v + end]));
    }
    None
}

fn unescape(s: &str) -> String {
    s.replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// Live's default User Library when `Library.cfg` doesn't say.
///
/// macOS: `~/Music/Ableton/User Library`. Windows:
/// `%USERPROFILE%\Documents\Ableton\User Library`.
pub fn default_user_library(os: Os, home: &str) -> Option<PathBuf> {
    if home.is_empty() {
        return None;
    }
    let parent = match os {
        Os::Macos => "Music",
        Os::Windows => "Documents",
    };
    Some(
        Path::new(home)
            .join(parent)
            .join("Ableton")
            .join("User Library"),
    )
}

/// Files under the User Library worth keeping in step. Sorted.
pub fn find_user_library_files(user_library: &Path) -> Vec<PathBuf> {
    let walk = walkdir::WalkDir::new(user_library)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            e.depth() == 0
                || !(name.starts_with('.')
                    || e.file_type().is_dir()
                        && SKIPPED_DIRS.contains(&name.to_lowercase().as_str()))
        });
    let mut out: Vec<PathBuf> = walk
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .filter(|e| {
            e.path()
                .strip_prefix(user_library)
                .ok()
                .and_then(rel_string)
                .is_some_and(|rel| user_library_label(&rel).is_some())
        })
        .filter(|e| e.metadata().is_ok_and(|m| m.len() <= MAX_FILE_BYTES))
        .map(walkdir::DirEntry::into_path)
        .collect();
    out.sort();
    out
}

/// What an Ableton item is, for the UI: "Template", "Collections", ...
pub fn describe(path: &PortablePath) -> &'static str {
    let label = match path {
        PortablePath::Logical { root, rel } if root == ABLETON_USER_LIBRARY => {
            user_library_label(rel)
        }
        PortablePath::Logical { root, rel } if root == ABLETON_PREFS => prefs_label(rel),
        _ => None,
    };
    label.unwrap_or("File")
}

/// What a User Library file is, or `None` when it must never be shared or
/// written: not a settings file, hidden, in a skipped folder, or not a
/// plain relative path.
pub fn user_library_label(rel: &str) -> Option<&'static str> {
    let parts = safe_parts(rel)?;
    let (file, dirs) = parts.split_last()?;
    if dirs
        .iter()
        .any(|d| SKIPPED_DIRS.contains(&d.to_lowercase().as_str()))
    {
        return None;
    }
    let lower = file.to_lowercase();
    let ext = lower.rsplit_once('.')?.1;
    let in_defaults = dirs
        .first()
        .is_some_and(|d| d.eq_ignore_ascii_case("Defaults"));
    Some(match ext {
        "xmp" => "Collections",
        "als" => "Template",
        "amxd" => "Max for Live device",
        "alc" => "Clip",
        "agr" => "Groove",
        "adg" | "adv" if in_defaults => "Default preset",
        "adg" => "Rack",
        "adv" => "Preset",
        _ => return None,
    })
}

/// "Setting" for the preferences files that are safe to share
/// (`Options.txt`), `None` for everything else in that folder.
pub fn prefs_label(rel: &str) -> Option<&'static str> {
    PORTABLE_PREFS
        .iter()
        .any(|f| f.eq_ignore_ascii_case(rel))
        .then_some("Setting")
}

/// The `/`-separated parts of a plain relative path: no `..`, `.`, empty,
/// hidden or drive parts, no backslashes.
fn safe_parts(rel: &str) -> Option<Vec<&str>> {
    if rel.contains(['\\', ':', '\0']) {
        return None;
    }
    let parts: Vec<&str> = rel.split('/').collect();
    parts
        .iter()
        .all(|p| !p.is_empty() && !p.starts_with('.'))
        .then_some(parts)
}

/// True for an item this module produced.
pub fn is_ableton_item(item: &Item) -> bool {
    item.product_id
        .as_ref()
        .is_some_and(|id| id.0.starts_with(PRODUCT_PREFIX))
}

/// Every shareable Ableton file as a manifest item.
///
/// Each is a portable product `ableton/<root>/<path>` with vendor
/// [`VENDOR`], its content hash (user-owned files) and no `requires` yet.
/// `.xmp` (Collections) and preferences are [`ItemKind::Config`]; the rest
/// [`ItemKind::DawAsset`]. Unreadable files are skipped.
pub fn settings_items(
    user_library: Option<&Path>,
    prefs: Option<&Path>,
    os: Os,
) -> (Vec<Product>, Vec<Item>) {
    let mut files: Vec<(&str, &Path, PathBuf)> = Vec::new();
    if let Some(lib) = user_library {
        files.extend(
            find_user_library_files(lib)
                .into_iter()
                .map(|p| (ABLETON_USER_LIBRARY, lib, p)),
        );
    }
    if let Some(prefs) = prefs {
        files.extend(
            PORTABLE_PREFS
                .iter()
                .map(|f| prefs.join(f))
                .filter(|p| p.is_file())
                .map(|p| (ABLETON_PREFS, prefs, p)),
        );
    }

    let mut products = Vec::new();
    let mut items = Vec::new();
    for (root, base, path) in files {
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let Ok(rel) = path.strip_prefix(base) else {
            continue;
        };
        let Some(rel) = rel_string(rel) else {
            continue;
        };
        let short = if root == ABLETON_PREFS {
            "prefs"
        } else {
            "lib"
        };
        let id = ProductId(format!("{PRODUCT_PREFIX}{short}/{}", slug(&rel)));
        let path = PortablePath::logical(root, rel.clone());
        let kind = if root == ABLETON_PREFS || describe(&path) == "Collections" {
            ItemKind::Config
        } else {
            ItemKind::DawAsset
        };
        let meta = std::fs::metadata(base.join(&rel)).ok();
        let hash = format!("blake3:{}", blake3::hash(&bytes).to_hex());
        products.push(Product {
            canonical_id: id.clone(),
            vendor: VENDOR.into(),
            name: rel,
        });
        items.push(Item {
            kind,
            product_id: Some(id),
            format: None,
            platform: os,
            arch: None,
            version: None,
            path,
            content_hash: ContentHash::try_from(hash).ok(),
            size: Some(bytes.len() as u64),
            mtime: meta
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64),
            platform_portable: true,
            confidence: Confidence::High,
            requires: Vec::new(),
        });
    }
    (products, items)
}

/// A relative path as `/`-separated UTF-8, or `None` if it isn't valid UTF-8.
fn rel_string(rel: &Path) -> Option<String> {
    let parts: Option<Vec<&str>> = rel.components().map(|c| c.as_os_str().to_str()).collect();
    Some(parts?.join("/"))
}

/// Lowercase alphanumeric words joined by `-`, keeping `/` between folders
/// so `Presets/A B.adg` and `Presets-A/B.adg` stay apart.
fn slug(s: &str) -> String {
    s.split('/')
        .map(|part| {
            part.split(|c: char| !c.is_alphanumeric())
                .filter(|w| !w.is_empty())
                .map(str::to_lowercase)
                .collect::<Vec<_>>()
                .join("-")
        })
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_folders() {
        assert_eq!(
            parse_live_folder("Live 12.1.5"),
            Some(("12.1.5".into(), false))
        );
        assert_eq!(
            parse_live_folder("Live 12.2b3 Beta"),
            Some(("12.2b3".into(), true))
        );
        assert_eq!(parse_live_folder("Live Reports"), None);
        assert_eq!(parse_live_folder("Max 8"), None);
    }

    #[test]
    fn prefs_bases() {
        assert_eq!(
            prefs_base(Os::Macos, "", "/Users/alice"),
            Some(PathBuf::from("/Users/alice/Library/Preferences/Ableton"))
        );
        assert!(prefs_base(Os::Windows, "", "/Users/alice").is_none());
    }

    #[test]
    fn user_library_paths_from_library_cfg() {
        let cfg = r#"<?xml version="1.0" encoding="UTF-8"?>
<Ableton MajorVersion="5" Creator="Ableton Live 12.1.5">
  <ContentLibrary>
    <UserLibrary>
      <LibraryProject>
        <ProjectPathX Value="nope" />
        <ProjectPath Value="/Users/alice/Music/Ableton/User Library" />
        <ProjectName Value="User Library" />
      </LibraryProject>
    </UserLibrary>
  </ContentLibrary>
</Ableton>"#;
        assert_eq!(
            user_library_from_cfg(cfg, Os::Macos),
            Some(PathBuf::from("/Users/alice/Music/Ableton/User Library"))
        );
        let parent_only = r#"<UserLibrary><LibraryProject>
            <ProjectName Value="User Library"/>
            <ProjectPath Value="D:\Music &amp; Stuff\Ableton\"/>
        </LibraryProject></UserLibrary>"#;
        assert_eq!(
            user_library_from_cfg(parent_only, Os::Windows),
            Some(PathBuf::from(r"D:\Music & Stuff\Ableton\User Library"))
        );
        assert_eq!(user_library_from_cfg("<Ableton/>", Os::Macos), None);
    }

    #[test]
    fn descriptions() {
        let lib = |rel: &str| describe(&PortablePath::logical(ABLETON_USER_LIBRARY, rel));
        assert_eq!(lib("Templates/Default.als"), "Template");
        assert_eq!(lib("Ableton Folder Info/12/Collections.xmp"), "Collections");
        assert_eq!(lib("Defaults/Audio Effects/EQ Eight.adv"), "Default preset");
        assert_eq!(lib("Presets/Instruments/Drum Rack/Kit.adg"), "Rack");
        assert_eq!(lib("Presets/Audio Effects/Reverb/Hall.adv"), "Preset");
        assert_eq!(
            describe(&PortablePath::logical(ABLETON_PREFS, "Options.txt")),
            "Setting"
        );
    }

    #[test]
    fn unsafe_or_private_paths_are_never_shared() {
        for rel in [
            "Samples/Imported/kick.wav",
            "Samples/Processed/x.adg",
            "Projects/Song Project/Backup/Song [2026].als",
            "../Preferences.cfg",
            "Presets/./x.adg",
            ".hidden/x.adg",
            "/abs/x.adg",
            r"Presets\x.adg",
            "Presets/notes.txt",
        ] {
            assert_eq!(user_library_label(rel), None, "{rel}");
        }
        assert_eq!(prefs_label("Preferences.cfg"), None);
        assert_eq!(prefs_label("Library.cfg"), None);
        assert_eq!(prefs_label("options.txt"), Some("Setting"));
    }

    #[test]
    fn slugs_keep_folders_apart() {
        assert_eq!(slug("Presets/A B.adg"), "presets/a-b-adg");
        assert_ne!(slug("Presets/A B.adg"), slug("Presets-A/B.adg"));
    }
}
