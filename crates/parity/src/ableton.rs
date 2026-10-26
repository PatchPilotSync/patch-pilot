//! Ableton Live settings and User Library files, for parity between machines.
//!
//! What's read, and why (see decision #41):
//!
//! - **User Library** (`ABLETON_USER_LIBRARY`): templates (`Templates/*.als`,
//!   including `Default.als`), default device presets (`Defaults/`), racks and
//!   presets (`.adg`, `.adv`), Max for Live devices (`.amxd`), clips (`.alc`),
//!   grooves (`.agr`), tunings (`.ascl`), control surface scripts
//!   (`Remote Scripts/**.py`), and Collections tags: the `.xmp` files Live
//!   writes to `Ableton Folder Info/` (Live 12 uses its `12/` subfolder).
//! - **Preferences folder** (`ABLETON_PREFS`): `Options.txt` only.
//!
//! Never read or shared: `Preferences.cfg` (binary, and it mixes portable
//! settings with the audio device, MIDI ports, folder paths, plug-in folders
//! and the Ableton login), samples, Packs, project `Backup/` folders.
//! `Library.cfg` holds the Collection names next to machine paths (User
//! Library, Places), so it is shared as its names and the Places inside a
//! cloud folder or the User Library, relative to it ([`library_cfg_text`]),
//! and received by rewriting just the `DisplayName` of each
//! `<LabelInfo Uri="color:colors=N">` and the `<UserFolderInfoList>`
//! (decision #79).
//!
//! Sets (`.als`) and racks (`.adg`) are gzipped XML; the plug-ins they use
//! are read from their `<VstPluginInfo>`, `<Vst3PluginInfo>` and
//! `<AuPluginInfo>` elements for readiness, like Reaper templates:
//!
//! ```text
//! <VstPluginInfo Id="0"> ... <PlugName Value="Acme Comp x64" /> <UniqueId Value="1094807365" />
//! <Vst3PluginInfo Id="0"> ... <Name Value="Acme Verb" /> <Uid> <Fields.0 Value="-1" /> ... </Uid>
//! <AuPluginInfo Id="0"> ... <Name Value="Acme Synth" /> <Manufacturer Value="Acme Audio" />
//! ```
//!
//! Each element's `<Preset>` child (saved plug-in state) is skipped.
//!
//! Live's preferences live in one folder per version: macOS
//! `~/Library/Preferences/Ableton/Live 12.1.5/`, Windows
//! `%APPDATA%\Ableton\Live 12.1.5\Preferences\`. The newest one wins.

use std::path::{Path, PathBuf};

use std::io::Read;

use patchpilot_core::{
    Confidence, ContentHash, Item, ItemKind, Os, PluginFormat, PortablePath, Product, ProductId,
};

use crate::reaper::PluginRef;
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

/// The User Library folder `Library.cfg` points at, if it says: the
/// `<ProjectPath Value="..."/>` inside `<UserLibrary>`, which is the folder
/// itself (Live 12 writes `/` even on Windows, and the folder needn't be
/// named "User Library").
pub fn user_library_from_cfg(cfg: &str) -> Option<PathBuf> {
    let start = cfg.find("<UserLibrary")?;
    let section = &cfg[start..];
    let section = section
        .find("</UserLibrary>")
        .map_or(section, |end| &section[..end]);
    let path = xml_value(section, "ProjectPath")?;
    let trimmed = path.trim_end_matches(['/', '\\']);
    (!trimmed.is_empty()).then(|| PathBuf::from(trimmed))
}

/// The file holding the Collection names, below [`ABLETON_PREFS`].
pub const LIBRARY_CFG: &str = "Library.cfg";

/// Live's seven Collection colours (`color:colors=1` to `7`).
const COLLECTIONS: std::ops::RangeInclusive<u8> = 1..=7;

/// The Collection names in `Library.cfg`, by colour number (1-7), from its
/// `<LabelInfo Uri="color:colors=N" DisplayName="..."/>` elements. Empty
/// names are kept (Live shows the colour's default). Colours without an
/// element are left out.
pub fn collection_names(cfg: &str) -> Vec<(u8, String)> {
    COLLECTIONS
        .filter_map(|n| {
            let elem = label_info(cfg, n)?;
            Some((n, unescape(&attr(&cfg[elem.clone()], "DisplayName")?.1)))
        })
        .collect()
}

/// The byte range of the `<LabelInfo .../>` element for colour `n`.
fn label_info(cfg: &str, n: u8) -> Option<std::ops::Range<usize>> {
    let uri = format!("Uri=\"color:colors={n}\"");
    let at = cfg.find(&uri)?;
    let start = cfg[..at].rfind('<')?;
    let end = at + cfg[at..].find('>')? + 1;
    cfg[start..]
        .starts_with("<LabelInfo ")
        .then_some(start..end)
}

/// The `name="value"` attribute in one element: the value's byte range in
/// `elem` and the raw (still escaped) value.
fn attr(elem: &str, name: &str) -> Option<(std::ops::Range<usize>, String)> {
    let key = format!(" {name}=\"");
    let v = elem.find(&key)? + key.len();
    let end = v + elem[v..].find('"')?;
    Some((v..end, elem[v..end].to_owned()))
}

/// What travels between machines for the Collection names: one
/// `<number>\t<name>` line per colour. Only names, never paths.
pub fn collection_names_text(names: &[(u8, String)]) -> String {
    names
        .iter()
        .map(|(n, name)| format!("{n}\t{}\n", name.replace(['\t', '\n', '\r'], " ")))
        .collect()
}

fn parse_collection_names_text(text: &str) -> Option<Vec<(u8, String)>> {
    text.lines()
        .map(|line| {
            let (n, name) = line.split_once('\t')?;
            let n: u8 = n.parse().ok()?;
            COLLECTIONS.contains(&n).then(|| (n, name.to_owned()))
        })
        .collect()
}

/// `cfg` with its Collection names set from `text` (as written by
/// [`collection_names_text`]). Only the `DisplayName` of each colour's
/// `<LabelInfo>` changes; the User Library location, Places and every other
/// byte stay as they are. Errors when `text` is malformed or `cfg` has no
/// Collections yet (Live writes them the first time its browser opens).
pub fn set_collection_names(cfg: &str, text: &str) -> Result<String, String> {
    let names = parse_collection_names_text(text).ok_or("Collection names are unreadable")?;
    let mut out = cfg.to_owned();
    for (n, name) in names {
        let elem = label_info(&out, n).ok_or(
            "Live hasn't written its Collections on this machine yet. Open Live once, then try again.",
        )?;
        let (value, _) = attr(&out[elem.clone()], "DisplayName")
            .ok_or("Library.cfg has a Collection without a name")?;
        out.replace_range(
            elem.start + value.start..elem.start + value.end,
            &escape(&name),
        );
    }
    Ok(out)
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// A folder each machine has under the same name, so Places inside it can
/// travel (decision #79): a cloud folder ("Dropbox", "OneDrive (Contoso)")
/// or the User Library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaceRoot {
    pub name: String,
    pub path: String,
}

/// The name Places inside the User Library travel under.
const USER_LIBRARY_PLACES: &str = "User Library";

/// The line in the shared text after which Places follow, one
/// `{Root}/path\t<name>` per line. Text from before Places travelled has
/// none, and leaves Places alone.
const PLACES_LINE: &str = "places";

/// A Place: a folder added to Live's browser sidebar, from a
/// `<UserFolderInfo Path="..." DisplayName="..."/>` in `Library.cfg`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    pub path: String,
    pub name: String,
    /// The element's bytes in the cfg.
    range: std::ops::Range<usize>,
}

/// Every Place in `cfg`, in the sidebar's order.
pub fn places(cfg: &str) -> Vec<Place> {
    let Some((_, Some(inner))) = places_list(cfg) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut from = inner.start;
    while let Some(at) = cfg[from..inner.end].find("<UserFolderInfo ") {
        let start = from + at;
        let Some(len) = cfg[start..inner.end].find('>') else {
            break;
        };
        let end = start + len + 1;
        let elem = &cfg[start..end];
        if let (Some((_, path)), Some((_, name))) = (attr(elem, "Path"), attr(elem, "DisplayName"))
        {
            out.push(Place {
                path: unescape(&path),
                name: unescape(&name),
                range: start..end,
            });
        }
        from = end;
    }
    out
}

/// The `<UserFolderInfoList>` element's bytes, and its insides unless it's
/// written `<UserFolderInfoList />`.
fn places_list(cfg: &str) -> Option<(std::ops::Range<usize>, Option<std::ops::Range<usize>>)> {
    const OPEN: &str = "<UserFolderInfoList";
    const CLOSE: &str = "</UserFolderInfoList>";
    let start = cfg.find(OPEN)?;
    if !cfg[start + OPEN.len()..].starts_with([' ', '/', '>']) {
        return None;
    }
    let gt = start + cfg[start..].find('>')?;
    if cfg[..gt].ends_with('/') {
        return Some((start..gt + 1, None));
    }
    let close = gt + cfg[gt..].find(CLOSE)?;
    Some((start..close + CLOSE.len(), Some(gt + 1..close)))
}

/// `/`-separated, without a trailing `/`.
fn norm(path: &str) -> String {
    path.replace('\\', "/").trim_end_matches('/').to_owned()
}

/// The folders Places can travel under on this machine: its cloud folders
/// and the User Library `cfg` names.
fn place_roots(cfg: &str, clouds: &[PlaceRoot]) -> Vec<PlaceRoot> {
    let mut roots = clouds.to_vec();
    if let Some(lib) = user_library_from_cfg(cfg) {
        roots.push(PlaceRoot {
            name: USER_LIBRARY_PLACES.into(),
            path: lib.to_string_lossy().into_owned(),
        });
    }
    roots
}

/// `path` as it travels: `{Dropbox}/Samples/Breaks`, below the most specific
/// root that holds it (case aside), or `None` for a folder only this machine
/// has, such as one on another drive.
fn portable_place(path: &str, roots: &[PlaceRoot]) -> Option<String> {
    let path = norm(path);
    let lower = path.to_lowercase();
    roots
        .iter()
        .filter_map(|r| {
            let base = norm(&r.path);
            let lower_base = base.to_lowercase();
            if base.is_empty() || !lower.starts_with(&lower_base) {
                return None;
            }
            let rest = path.get(base.len()..)?;
            (rest.is_empty() || rest.starts_with('/'))
                .then(|| (base.len(), format!("{{{}}}{rest}", r.name)))
        })
        .max_by_key(|(len, _)| *len)
        .map(|(_, p)| p)
}

/// Where a travelling Place is on this machine, spelled with `\` when
/// `backslash`; `None` when its root isn't here.
fn local_place(portable: &str, roots: &[PlaceRoot], backslash: bool) -> Option<String> {
    let (name, rest) = portable.strip_prefix('{')?.split_once('}')?;
    if !(rest.is_empty() || rest.starts_with('/')) || rest.split('/').any(|p| p == "..") {
        return None;
    }
    let root = roots.iter().find(|r| r.name == name)?;
    let path = format!("{}{rest}", norm(&root.path));
    Some(if backslash {
        path.replace('/', "\\")
    } else {
        path
    })
}

/// The travelling Places in shared text, as (`{Root}/path`, name); `None`
/// for text from before Places travelled.
fn parse_places_text(text: &str) -> Option<Vec<(String, String)>> {
    let at = text.lines().position(|l| l == PLACES_LINE)?;
    Some(
        text.lines()
            .skip(at + 1)
            .filter_map(|l| l.split_once('\t'))
            .map(|(p, n)| (p.to_owned(), n.to_owned()))
            .collect(),
    )
}

/// What travels for `Library.cfg`: the Collection names, then the Places
/// inside a cloud folder or the User Library, with paths relative to it.
/// Empty for a file without Collections.
pub fn library_cfg_text(cfg: &str, clouds: &[PlaceRoot]) -> String {
    let names = collection_names(cfg);
    if names.is_empty() {
        return String::new();
    }
    let roots = place_roots(cfg, clouds);
    let mut text = collection_names_text(&names);
    text.push_str(PLACES_LINE);
    text.push('\n');
    for p in places(cfg) {
        if let Some(portable) = portable_place(&p.path, &roots) {
            let name = p.name.replace(['\t', '\n', '\r'], " ");
            text.push_str(&format!("{portable}\t{name}\n"));
        }
    }
    text
}

/// `cfg` with the Collection names and Places from `text` (as written by
/// [`library_cfg_text`]). Travelling Places take the incoming order and
/// names, each pointed at the same folder here; one whose root this machine
/// lacks is left out ([`places_not_here`] lists it). Places only this
/// machine has (another drive) stay, after them. Nothing else changes.
pub fn set_library_cfg(
    cfg: &str,
    text: &str,
    clouds: &[PlaceRoot],
    os: Os,
) -> Result<String, String> {
    let names: String = text
        .lines()
        .take_while(|l| *l != PLACES_LINE)
        .map(|l| format!("{l}\n"))
        .collect();
    let cfg = set_collection_names(cfg, &names)?;
    match parse_places_text(text) {
        Some(incoming) => Ok(set_places(&cfg, &incoming, clouds, os)),
        None => Ok(cfg),
    }
}

fn set_places(cfg: &str, incoming: &[(String, String)], clouds: &[PlaceRoot], os: Os) -> String {
    let Some((whole, _)) = places_list(cfg) else {
        // Live writes the list with the rest; without it, leave Places be.
        return cfg.to_owned();
    };
    let roots = place_roots(cfg, clouds);
    let existing = places(cfg);
    // Live 12 writes `\` on Windows; follow what the file already does.
    let backslash = existing
        .iter()
        .find_map(|p| {
            if p.path.contains('\\') {
                Some(true)
            } else {
                p.path.contains('/').then_some(false)
            }
        })
        .unwrap_or(os == Os::Windows);
    let mut next_id = max_id(cfg) + 1;
    let same = |a: &str, b: &str| norm(a).to_lowercase() == norm(b).to_lowercase();
    let mut used = vec![None; existing.len()];
    let mut shared: Vec<String> = Vec::new();
    for (portable, name) in incoming {
        let Some(path) = local_place(portable, &roots, backslash) else {
            continue;
        };
        let found =
            (0..existing.len()).find(|&i| used[i].is_none() && same(&existing[i].path, &path));
        match found {
            Some(i) => {
                used[i] = Some(shared.len());
                let p = &existing[i];
                let mut elem = cfg[p.range.clone()].to_owned();
                if p.name != *name
                    && let Some((value, _)) = attr(&elem, "DisplayName")
                {
                    elem.replace_range(value, &escape(name));
                }
                shared.push(elem);
            }
            None => {
                shared.push(format!(
                    r#"<UserFolderInfo Id="{next_id}" Path="{}" DisplayName="{}" IconName="" />"#,
                    escape(&path),
                    escape(name)
                ));
                next_id += 1;
            }
        }
    }
    // This machine's own Places (outside the shared folders) stay, each
    // after the Place it followed.
    let mut after: Vec<Vec<String>> = vec![Vec::new(); shared.len() + 1];
    for (j, p) in existing.iter().enumerate() {
        if used[j].is_none() && portable_place(&p.path, &roots).is_none() {
            let slot = (0..j).rev().find_map(|i| used[i]).map_or(0, |k| k + 1);
            after[slot].push(cfg[p.range.clone()].to_owned());
        }
    }
    let mut after = after.into_iter();
    let mut elems: Vec<String> = after.next().unwrap_or_default();
    for (elem, own) in shared.into_iter().zip(after) {
        elems.push(elem);
        elems.extend(own);
    }
    let before: Vec<&str> = existing.iter().map(|p| &cfg[p.range.clone()]).collect();
    if elems == before {
        return cfg.to_owned();
    }
    let nl = if cfg.contains("\r\n") { "\r\n" } else { "\n" };
    let line_start = cfg[..whole.start].rfind('\n').map_or(0, |n| n + 1);
    let indent = &cfg[line_start..whole.start];
    let indent = if indent.trim().is_empty() { indent } else { "" };
    let inner_indent = match existing.first() {
        Some(p) => {
            let s = cfg[..p.range.start].rfind('\n').map_or(0, |n| n + 1);
            let i = &cfg[s..p.range.start];
            if i.trim().is_empty() {
                i.to_owned()
            } else {
                format!("{indent}\t")
            }
        }
        None => format!("{indent}\t"),
    };
    let list = if elems.is_empty() {
        "<UserFolderInfoList />".to_owned()
    } else {
        let body: String = elems
            .iter()
            .map(|e| format!("{nl}{inner_indent}{e}"))
            .collect();
        format!("<UserFolderInfoList>{body}{nl}{indent}</UserFolderInfoList>")
    };
    let mut out = cfg.to_owned();
    out.replace_range(whole, &list);
    out
}

/// The biggest `Id="N"` in `cfg`, so a new Place's is unique.
fn max_id(cfg: &str) -> u64 {
    cfg.match_indices(" Id=\"")
        .filter_map(|(at, m)| {
            let v = &cfg[at + m.len()..];
            v[..v.find('"')?].parse().ok()
        })
        .max()
        .unwrap_or(0)
}

/// The names of the travelling Places in shared `text` (another machine's)
/// whose folder isn't on this machine: its root isn't here, or the folder
/// inside it isn't there (yet). `cfg` is this machine's `Library.cfg`.
pub fn places_not_here(text: &str, cfg: &str, clouds: &[PlaceRoot]) -> Vec<String> {
    let roots = place_roots(cfg, clouds);
    parse_places_text(text)
        .unwrap_or_default()
        .into_iter()
        .filter(|(p, _)| local_place(p, &roots, false).is_none_or(|l| !Path::new(&l).is_dir()))
        .map(|(_, name)| name)
        .collect()
}

/// The bytes another machine gets for a shared Ableton file: the file
/// itself, except `Library.cfg`, which shares only its Collection names and
/// the Places in `clouds` (this machine's cloud folders) or the User Library.
pub fn shared_view(root: &str, rel: &str, bytes: Vec<u8>, clouds: &[PlaceRoot]) -> Vec<u8> {
    if root == ABLETON_PREFS && rel.eq_ignore_ascii_case(LIBRARY_CFG) {
        library_cfg_text(&String::from_utf8_lossy(&bytes), clouds).into_bytes()
    } else {
        bytes
    }
}

/// True for shared `Library.cfg` text from a version before Places travelled.
pub fn is_names_only(text: &[u8]) -> bool {
    parse_places_text(&String::from_utf8_lossy(text)).is_none()
}

/// What to write for a shared Ableton file given what's there now: the
/// incoming bytes, except for `Library.cfg`, where only the Collection names
/// and Places are merged into this machine's own file.
pub fn merge_incoming(
    root: &str,
    rel: &str,
    current: Option<&[u8]>,
    incoming: Vec<u8>,
    clouds: &[PlaceRoot],
    os: Os,
) -> Result<Vec<u8>, String> {
    if root == ABLETON_PREFS && rel.eq_ignore_ascii_case(LIBRARY_CFG) {
        let cfg = current.ok_or(
            "Live hasn't written its settings on this machine yet. Open Live once, then try again.",
        )?;
        let cfg = std::str::from_utf8(cfg).map_err(|_| "Library.cfg isn't UTF-8")?;
        let text = std::str::from_utf8(&incoming).map_err(|_| "Collection names aren't UTF-8")?;
        set_library_cfg(cfg, text, clouds, os).map(String::into_bytes)
    } else {
        Ok(incoming)
    }
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

/// The insides of every `<tag ...>...</tag>` (not nested in itself).
fn xml_blocks<'a>(xml: &'a str, tag: &str) -> Vec<&'a str> {
    let (open, close) = (format!("<{tag}"), format!("</{tag}>"));
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(at) = xml[from..].find(&open) {
        let start = from + at + open.len();
        from = start;
        if !xml[start..].starts_with(|c: char| c.is_whitespace() || c == '>') {
            continue;
        }
        let Some(end) = xml[start..].find(&close) else {
            break;
        };
        out.push(&xml[start..start + end]);
        from = start + end + close.len();
    }
    out
}

/// `block` without its `<Preset>...</Preset>` child, which holds saved
/// plug-in state with names of its own.
fn without_preset(block: &str) -> std::borrow::Cow<'_, str> {
    match (block.find("<Preset>"), block.find("</Preset>")) {
        (Some(a), Some(b)) if a < b => {
            format!("{}{}", &block[..a], &block[b + "</Preset>".len()..]).into()
        }
        _ => block.into(),
    }
}

/// Live sets and racks are gzipped XML; files bigger than this unpacked are
/// skipped rather than read into memory.
const MAX_XML_BYTES: u64 = 256 * 1024 * 1024;

/// The plug-ins a Live set (`.als`) or rack (`.adg`) uses, unique and
/// sorted. Takes the file as Live writes it (gzipped) or plain XML;
/// anything else gives nothing.
pub fn parse_live_file(bytes: &[u8]) -> Vec<PluginRef> {
    if !bytes.starts_with(&[0x1f, 0x8b]) {
        return parse_live_xml(&String::from_utf8_lossy(bytes));
    }
    let mut xml = Vec::new();
    let read = flate2::read::MultiGzDecoder::new(bytes)
        .take(MAX_XML_BYTES + 1)
        .read_to_end(&mut xml);
    match read {
        Ok(n) if (n as u64) <= MAX_XML_BYTES => parse_live_xml(&String::from_utf8_lossy(&xml)),
        _ => Vec::new(),
    }
}

/// The plug-ins in Live's XML, unique and sorted. Malformed entries are
/// skipped.
pub fn parse_live_xml(xml: &str) -> Vec<PluginRef> {
    let mut out = Vec::new();
    for block in xml_blocks(xml, "VstPluginInfo") {
        let block = without_preset(block);
        if let Some(name) = xml_value(&block, "PlugName").filter(|n| !n.is_empty()) {
            out.push(PluginRef {
                format: PluginFormat::Vst2,
                key: xml_value(&block, "UniqueId").unwrap_or_else(|| name.clone()),
                name,
                vendor: None,
            });
        }
    }
    for block in xml_blocks(xml, "Vst3PluginInfo") {
        let block = without_preset(block);
        if let Some(name) = xml_value(&block, "Name").filter(|n| !n.is_empty()) {
            out.push(PluginRef {
                format: PluginFormat::Vst3,
                key: vst3_class_id(&block).unwrap_or_else(|| name.clone()),
                name,
                vendor: None,
            });
        }
    }
    for block in xml_blocks(xml, "AuPluginInfo") {
        let block = without_preset(block);
        if let Some(name) = xml_value(&block, "Name").filter(|n| !n.is_empty()) {
            let vendor = xml_value(&block, "Manufacturer").filter(|v| !v.is_empty());
            out.push(PluginRef {
                format: PluginFormat::Au,
                key: match &vendor {
                    Some(v) => format!("{v}: {name}"),
                    None => name.clone(),
                },
                name,
                vendor,
            });
        }
    }
    out.sort_by(|a, b| (a.format as u8, &a.key).cmp(&(b.format as u8, &b.key)));
    out.dedup_by(|a, b| a.format == b.format && a.key == b.key);
    out
}

/// The 32-hex-digit VST3 class ID from `<Uid><Fields.0 Value="..."/>`...
/// (four signed 32-bit words), as Reaper and the VST3 reader write it.
fn vst3_class_id(block: &str) -> Option<String> {
    let uid = xml_blocks(block, "Uid").into_iter().next()?;
    (0..4)
        .map(|i| {
            let v: i64 = xml_value(uid, &format!("Fields.{i}"))?.parse().ok()?;
            let word = i32::try_from(v)
                .map(|w| w as u32)
                .or_else(|_| u32::try_from(v))
                .ok()?;
            Some(format!("{word:08X}"))
        })
        .collect()
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
    if dirs.iter().any(|d| {
        let d = d.to_lowercase();
        SKIPPED_DIRS.contains(&d.as_str()) || d == "__pycache__"
    }) {
        return None;
    }
    let lower = file.to_lowercase();
    let ext = lower.rsplit_once('.')?.1;
    let in_defaults = dirs
        .first()
        .is_some_and(|d| d.eq_ignore_ascii_case("Defaults"));
    let in_remote_scripts = dirs
        .first()
        .is_some_and(|d| d.eq_ignore_ascii_case("Remote Scripts"));
    Some(match ext {
        // Control surface scripts Live loads from the User Library; compiled
        // `.pyc` files are Live's own cache.
        "py" if in_remote_scripts => "Control surface script",
        "ascl" => "Tuning",
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
/// (`Options.txt`), "Collection names and Places" for `Library.cfg` (only
/// those travel, see [`shared_view`]), `None` for everything else there.
pub fn prefs_label(rel: &str) -> Option<&'static str> {
    if rel.eq_ignore_ascii_case(LIBRARY_CFG) {
        return Some("Collection names and Places");
    }
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
/// [`VENDOR`] and its content hash (user-owned files). Sets and racks
/// require the products their plug-ins map to through `product_of`
/// (`None` leaves a plug-in out).
/// `.xmp` (Collections) and preferences are [`ItemKind::Config`]; the rest
/// [`ItemKind::DawAsset`]. Unreadable files are skipped.
pub fn settings_items(
    user_library: Option<&Path>,
    prefs: Option<&Path>,
    clouds: &[PlaceRoot],
    os: Os,
    product_of: impl Fn(&PluginRef) -> Option<ProductId>,
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
                .chain([&LIBRARY_CFG])
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
        let bytes = shared_view(root, &rel, bytes, clouds);
        if bytes.is_empty() {
            // A Library.cfg without Collections.
            continue;
        }
        let short = if root == ABLETON_PREFS {
            "prefs"
        } else {
            "lib"
        };
        let collections = root == ABLETON_PREFS && rel.eq_ignore_ascii_case(LIBRARY_CFG);
        let id = if collections {
            ProductId(format!("{PRODUCT_PREFIX}prefs/collection-names"))
        } else {
            ProductId(format!("{PRODUCT_PREFIX}{short}/{}", slug(&rel)))
        };
        let path = PortablePath::logical(root, rel.clone());
        let kind = if root == ABLETON_PREFS || describe(&path) == "Collections" {
            ItemKind::Config
        } else {
            ItemKind::DawAsset
        };
        let mut requires: Vec<ProductId> = if has_plugins(&rel) {
            parse_live_file(&bytes)
                .iter()
                .filter_map(&product_of)
                .collect()
        } else {
            Vec::new()
        };
        requires.sort();
        requires.dedup();
        let meta = std::fs::metadata(base.join(&rel)).ok();
        let hash = format!("blake3:{}", blake3::hash(&bytes).to_hex());
        products.push(Product {
            canonical_id: id.clone(),
            vendor: VENDOR.into(),
            name: if collections {
                "Collection names and Places".to_owned()
            } else {
                rel
            },
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
            requires,
            needs_files: Vec::new(),
        });
    }
    (products, items)
}

/// Sets and racks can hold plug-ins; presets of Live's own devices can't.
fn has_plugins(rel: &str) -> bool {
    let lower = rel.to_lowercase();
    lower.ends_with(".als") || lower.ends_with(".adg")
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
            user_library_from_cfg(cfg),
            Some(PathBuf::from("/Users/alice/Music/Ableton/User Library"))
        );
        // Live 12 on Windows: forward slashes, and a folder with its own name.
        let renamed = r#"<UserLibrary><LibraryProject>
            <ProjectName Value="User Library"/>
            <ProjectPath Value="D:/Dropbox/Music &amp; Stuff/!User Library/"/>
        </LibraryProject></UserLibrary>"#;
        assert_eq!(
            user_library_from_cfg(renamed),
            Some(PathBuf::from("D:/Dropbox/Music & Stuff/!User Library"))
        );
        assert_eq!(user_library_from_cfg("<Ableton/>"), None);
    }

    #[test]
    fn descriptions() {
        let lib = |rel: &str| describe(&PortablePath::logical(ABLETON_USER_LIBRARY, rel));
        assert_eq!(lib("Templates/Default.als"), "Template");
        assert_eq!(lib("Ableton Folder Info/12/Collections.xmp"), "Collections");
        assert_eq!(lib("Defaults/Audio Effects/EQ Eight.adv"), "Default preset");
        assert_eq!(lib("Presets/Instruments/Drum Rack/Kit.adg"), "Rack");
        assert_eq!(lib("Presets/Audio Effects/Reverb/Hall.adv"), "Preset");
        assert_eq!(lib("Tunings/Just Intonation.ascl"), "Tuning");
        assert_eq!(
            lib("Remote Scripts/My Controller/__init__.py"),
            "Control surface script"
        );
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
            "Presets/helper.py",
            "Remote Scripts/My Controller/__init__.pyc",
            "Remote Scripts/My Controller/__pycache__/x.py",
        ] {
            assert_eq!(user_library_label(rel), None, "{rel}");
        }
        assert_eq!(prefs_label("Preferences.cfg"), None);
        assert_eq!(
            prefs_label("Library.cfg"),
            Some("Collection names and Places")
        );
        assert_eq!(prefs_label("Log.txt"), None);
        assert_eq!(prefs_label("options.txt"), Some("Setting"));
    }

    #[test]
    fn slugs_keep_folders_apart() {
        assert_eq!(slug("Presets/A B.adg"), "presets/a-b-adg");
        assert_ne!(slug("Presets/A B.adg"), slug("Presets-A/B.adg"));
    }

    #[test]
    fn gzipped_sets_are_read_and_junk_gives_nothing() {
        use std::io::Write;
        let xml = r#"<Ableton><VstPluginInfo Id="0"><PlugName Value="Acme &amp; Co" /></VstPluginInfo></Ableton>"#;
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(xml.as_bytes()).unwrap();
        let refs = parse_live_file(&gz.finish().unwrap());
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].name, "Acme & Co");
        assert!(parse_live_file(&[0x1f, 0x8b, 0, 1, 2]).is_empty());
        assert!(parse_live_file(b"fake rack").is_empty());
    }
}
