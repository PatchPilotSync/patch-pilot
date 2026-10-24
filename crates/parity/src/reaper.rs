//! Which plug-ins Reaper templates depend on, for readiness.
//!
//! Track templates (`.RTrackTemplate`), project templates (`.RPP`) and FX
//! chains (`.RfxChain`) are text. Each plug-in instance is a block opened by
//! a line like:
//!
//! ```text
//! <VST "VST3: Acme Verb (Acme Audio)" "Acme Verb.vst3" 0 "" 1997878177{5653544163654D6163726F7665726200} ""
//! <VST "VST: Acme Comp (Acme Audio)" "Acme Comp x64.dll" 0 "" 1094807365<56535441434D4F4D> ""
//! <AU "AU: Acme Synth (Acme Audio)" "Acme Audio: Acme Synth" "" 1635085685<...>
//! <CLAP "CLAP: Acme Delay (Acme Audio)" com.acmeaudio.delay ""
//! <JS "Acme/gate.jsfx" ""
//! ```
//!
//! Only those opening lines are read; plug-in state (base64) is skipped.

use std::path::{Path, PathBuf};

use patchpilot_core::{
    Confidence, ContentHash, Item, ItemKind, Os, PluginFormat, PortablePath, Product, ProductId,
};

/// One plug-in reference in a template.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PluginRef {
    pub format: PluginFormat,
    /// Name without the `VST3:` prefix or `(Vendor)` suffix.
    pub name: String,
    pub vendor: Option<String>,
    /// VST3 class ID, CLAP ID, AU `Vendor: Name`, JSFX path, or VST2 file name.
    pub key: String,
}

impl PluginRef {
    /// A stable identity hint for identity resolution, e.g. `vst3:<CID>`,
    /// `clap:com.vendor.id`, `au:Vendor: Name`, `jsfx:Acme/gate.jsfx`,
    /// `vst2:Acme Comp x64.dll`.
    pub fn identity_hint(&self) -> String {
        let prefix = match self.format {
            PluginFormat::Vst2 => "vst2",
            PluginFormat::Vst3 => "vst3",
            PluginFormat::Au => "au",
            PluginFormat::Aax => "aax",
            PluginFormat::Clap => "clap",
            PluginFormat::Jsfx => "jsfx",
        };
        format!("{prefix}:{}", self.key)
    }

    /// Ships with Reaper, so every Reaper install has it: JSFX, and Cockos's
    /// own plug-ins (ReaEQ, ReaComp, ...), which live in Reaper's program
    /// folder rather than the plug-in folders we scan.
    pub fn is_stock(&self) -> bool {
        self.format == PluginFormat::Jsfx
            || self
                .vendor
                .as_deref()
                .is_some_and(|v| v.trim().eq_ignore_ascii_case("cockos"))
    }
}

/// The plug-ins one template file depends on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateDeps {
    pub template: PathBuf,
    /// Unique by identity and sorted; a plug-in used on five tracks appears once.
    pub plugins: Vec<PluginRef>,
}

/// Parse template text. Unknown or malformed plug-in lines are skipped.
pub fn parse_template(text: &str) -> Vec<PluginRef> {
    let mut refs: Vec<PluginRef> = text.lines().filter_map(parse_line).collect();
    refs.sort_by_cached_key(|r| (r.identity_hint(), r.name.clone()));
    refs.dedup_by(|a, b| a.identity_hint() == b.identity_hint());
    refs
}

fn parse_line(line: &str) -> Option<PluginRef> {
    let tokens = tokenize(line.trim_start());
    let (tag, rest) = tokens.split_first()?;
    match tag.as_str() {
        "<VST" => {
            let display = rest.first()?;
            let (format, name, vendor) = split_display(display)?;
            let key = match format {
                PluginFormat::Vst3 => rest
                    .iter()
                    .find_map(|t| braced_cid(t))
                    .or_else(|| rest.get(1).cloned())?,
                _ => rest.get(1)?.clone(),
            };
            Some(PluginRef {
                format,
                name,
                vendor,
                key,
            })
        }
        "<AU" => {
            let (_, name, vendor) = split_display(rest.first()?)?;
            let key = rest.get(1).cloned().unwrap_or_else(|| name.clone());
            Some(PluginRef {
                format: PluginFormat::Au,
                name,
                vendor,
                key,
            })
        }
        "<CLAP" => {
            let (_, name, vendor) = split_display(rest.first()?)?;
            Some(PluginRef {
                format: PluginFormat::Clap,
                name,
                vendor,
                key: rest.get(1)?.clone(),
            })
        }
        "<JS" => {
            let path = rest.first()?.clone();
            if path.is_empty() {
                return None;
            }
            let name = path
                .rsplit('/')
                .next()
                .unwrap_or(&path)
                .trim_end_matches(".jsfx")
                .to_owned();
            Some(PluginRef {
                format: PluginFormat::Jsfx,
                name,
                vendor: None,
                key: path,
            })
        }
        _ => None,
    }
}

/// `VST3: Acme Verb (Acme Audio)` → (VST3, "Acme Verb", Some("Acme Audio")).
/// `VSTi:` / `VST3i:` / `AUi:` / `CLAPi:` are instruments of the same format.
fn split_display(display: &str) -> Option<(PluginFormat, String, Option<String>)> {
    let (prefix, rest) = display.split_once(':')?;
    let format = match prefix.trim().to_ascii_uppercase().as_str() {
        "VST" | "VSTI" => PluginFormat::Vst2,
        "VST3" | "VST3I" => PluginFormat::Vst3,
        "AU" | "AUI" => PluginFormat::Au,
        "CLAP" | "CLAPI" => PluginFormat::Clap,
        _ => return None,
    };
    let rest = rest.trim();
    let (name, vendor) = match rest.strip_suffix(')').and_then(|r| r.rsplit_once(" (")) {
        Some((n, v)) if !v.is_empty() => (n.trim().to_owned(), Some(v.to_owned())),
        _ => (rest.to_owned(), None),
    };
    Some((format, name, vendor))
}

/// `1997878177{5653...}` → `5653...` uppercased, if it holds 32 hex chars.
fn braced_cid(token: &str) -> Option<String> {
    let inner = token.split_once('{')?.1.strip_suffix('}')?;
    (inner.len() == 32 && inner.chars().all(|c| c.is_ascii_hexdigit()))
        .then(|| inner.to_ascii_uppercase())
}

/// Split on whitespace, honouring Reaper's quote characters (`"`, `'`, `` ` ``).
fn tokenize(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = line.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
            continue;
        }
        let mut tok = String::new();
        if matches!(c, '"' | '\'' | '`') {
            chars.next();
            for ch in chars.by_ref() {
                if ch == c {
                    break;
                }
                tok.push(ch);
            }
        } else {
            while let Some(&ch) = chars.peek() {
                if ch.is_whitespace() {
                    break;
                }
                tok.push(ch);
                chars.next();
            }
        }
        out.push(tok);
    }
    out
}

/// Reaper's resource folder for a standard (non-portable) install.
///
/// Windows: `%APPDATA%\REAPER`. macOS: `~/Library/Application Support/REAPER`.
pub fn reaper_resource_dir(os: Os, appdata: &str, home: &str) -> Option<PathBuf> {
    match os {
        Os::Windows if !appdata.is_empty() => Some(Path::new(appdata).join("REAPER")),
        Os::Macos if !home.is_empty() => Some(
            Path::new(home)
                .join("Library")
                .join("Application Support")
                .join("REAPER"),
        ),
        _ => None,
    }
}

/// A user-owned file in Reaper's resource folder that Patch Pilot tracks and
/// can bring over to another machine (decision #45). Mirrored in `app/src/api.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    TrackTemplate,
    ProjectTemplate,
    FxChain,
    /// Anything under `Scripts/` (ReaScripts and the files they load,
    /// including ReaPack-installed ones).
    Script,
    /// Anything under `ColorThemes/`.
    Theme,
    /// `reaper-kb.ini`: shortcuts and custom actions. Reaper's own settings,
    /// so only written while Reaper is closed.
    KeyMap,
    /// `reaper-menu.ini`: customized menus and toolbars. Only written while
    /// Reaper is closed.
    Menus,
}

impl ResourceKind {
    /// The subfolder of the resource folder it lives in, or for a
    /// single-file kind, the file's name there.
    pub fn folder(self) -> &'static str {
        match self {
            Self::TrackTemplate => "TrackTemplates",
            Self::ProjectTemplate => "ProjectTemplates",
            Self::FxChain => "FXChains",
            Self::Script => "Scripts",
            Self::Theme => "ColorThemes",
            Self::KeyMap => "reaper-kb.ini",
            Self::Menus => "reaper-menu.ini",
        }
    }

    /// One file at the top of the resource folder rather than a folder.
    pub fn is_single_file(self) -> bool {
        matches!(self, Self::KeyMap | Self::Menus)
    }

    /// Reaper keeps it open and rewrites it, so it may only be written while
    /// Reaper is closed (CLAUDE.md rule 5).
    pub fn needs_reaper_closed(self) -> bool {
        self.is_single_file()
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::TrackTemplate => "Track template",
            Self::ProjectTemplate => "Project template",
            Self::FxChain => "FX chain",
            Self::Script => "Script",
            Self::Theme => "Theme",
            Self::KeyMap => "Key map",
            Self::Menus => "Menus and toolbars",
        }
    }

    /// Templates name the plug-ins they use; scripts and themes don't.
    pub fn is_template(self) -> bool {
        matches!(
            self,
            Self::TrackTemplate | Self::ProjectTemplate | Self::FxChain
        )
    }

    fn template_ext(self) -> Option<&'static str> {
        match self {
            Self::TrackTemplate => Some("rtracktemplate"),
            Self::ProjectTemplate => Some("rpp"),
            Self::FxChain => Some("rfxchain"),
            Self::Script | Self::Theme | Self::KeyMap | Self::Menus => None,
        }
    }

    fn product_prefix(self) -> &'static str {
        match self {
            Self::Script => "reaper-script",
            Self::Theme => "reaper-theme",
            Self::KeyMap | Self::Menus => "reaper-config",
            _ => "reaper-template",
        }
    }
}

const RESOURCE_KINDS: [ResourceKind; 7] = [
    ResourceKind::TrackTemplate,
    ResourceKind::ProjectTemplate,
    ResourceKind::FxChain,
    ResourceKind::Script,
    ResourceKind::Theme,
    ResourceKind::KeyMap,
    ResourceKind::Menus,
];

/// Compiled code is never brought over, even if it sits in `Scripts/`: it is
/// per-OS and is software, not a user file (CLAUDE.md rule 2).
const NEVER_SHARED_EXT: [&str; 8] = ["dll", "dylib", "so", "exe", "app", "msi", "pkg", "dmg"];

/// What a path below the resource folder (`/`-separated) is, if Patch Pilot
/// tracks it: a template with its extension in its folder, or any file in
/// `Scripts/` or `ColorThemes/` except compiled code and hidden files. `None`
/// for anything else, including paths that would leave the folder. Checked
/// before writing a file that came from another machine.
pub fn resource_kind(rel: &str) -> Option<ResourceKind> {
    let parts: Vec<&str> = rel.split('/').collect();
    if let [name] = parts[..] {
        return RESOURCE_KINDS
            .into_iter()
            .find(|k| k.is_single_file() && k.folder().eq_ignore_ascii_case(name));
    }
    let safe = parts.len() >= 2
        && parts.iter().all(|p| {
            !p.is_empty() && !p.starts_with('.') && !p.contains(['\\', ':']) && *p != "__pycache__"
        });
    if !safe {
        return None;
    }
    let kind = RESOURCE_KINDS
        .into_iter()
        .find(|k| !k.is_single_file() && k.folder().eq_ignore_ascii_case(parts[0]))?;
    let ext = Path::new(parts[parts.len() - 1])
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase());
    match kind.template_ext() {
        Some(want) => (ext.as_deref() == Some(want)).then_some(kind),
        None => (!ext.is_some_and(|e| NEVER_SHARED_EXT.contains(&e.as_str()))).then_some(kind),
    }
}

/// Every file under a resource folder that [`resource_kind`] tracks, with
/// its `/`-separated path below the folder. Sorted by path.
pub fn find_resources(resource_dir: &Path) -> Vec<(PathBuf, String, ResourceKind)> {
    let mut out = Vec::new();
    for kind in RESOURCE_KINDS {
        if kind.is_single_file() {
            let path = resource_dir.join(kind.folder());
            if path.is_file() {
                out.push((path, kind.folder().to_owned(), kind));
            }
            continue;
        }
        let walk = walkdir::WalkDir::new(resource_dir.join(kind.folder()))
            .follow_links(true)
            .into_iter()
            // Don't descend into .git, __pycache__ and the like.
            .filter_entry(|e| {
                e.depth() == 0 || {
                    let n = e.file_name().to_string_lossy();
                    !n.starts_with('.') && n != "__pycache__"
                }
            });
        for entry in walk.filter_map(Result::ok) {
            if !entry.file_type().is_file() {
                continue;
            }
            let Ok(rel) = entry.path().strip_prefix(resource_dir) else {
                continue;
            };
            let rel: Vec<_> = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect();
            let rel = rel.join("/");
            if resource_kind(&rel) == Some(kind) {
                out.push((entry.into_path(), rel, kind));
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Template files under a resource folder: `TrackTemplates/**.RTrackTemplate`,
/// `ProjectTemplates/**.RPP` and `FXChains/**.RfxChain`. Sorted.
pub fn find_templates(resource_dir: &Path) -> Vec<PathBuf> {
    find_resources(resource_dir)
        .into_iter()
        .filter(|(_, _, k)| k.is_template())
        .map(|(p, _, _)| p)
        .collect()
}

/// Parse every template under a resource folder. Unreadable files are skipped.
pub fn scan_templates(resource_dir: &Path) -> Vec<TemplateDeps> {
    find_templates(resource_dir)
        .into_iter()
        .filter_map(|template| {
            let bytes = std::fs::read(&template).ok()?;
            // Templates are UTF-8 in practice; tolerate stray bytes.
            let text = String::from_utf8_lossy(&bytes);
            Some(TemplateDeps {
                plugins: parse_template(&text),
                template,
            })
        })
        .collect()
}

/// Logical root for Reaper's resource folder in manifests.
pub const REAPER_RESOURCES: &str = "REAPER_RESOURCES";

/// Every tracked file under a resource folder as a manifest item (#37, #45).
///
/// Each is a portable `daw_asset` product `reaper-template/<path>` (or
/// `reaper-script/`, `reaper-theme/`) at `REAPER_RESOURCES/<rel>`, with its
/// content hash (they are user-owned files). Templates also get `requires`
/// set to the products they use: `product_of` maps a plug-in reference to a
/// product ID, or `None` to leave it out (e.g. stock JSFX that ship with
/// Reaper). Unreadable files are skipped.
pub fn template_items(
    resource_dir: &Path,
    os: Os,
    product_of: impl Fn(&PluginRef) -> Option<ProductId>,
) -> (Vec<Product>, Vec<Item>) {
    let mut products = Vec::new();
    let mut items = Vec::new();
    for (path, rel, kind) in find_resources(resource_dir) {
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let id = ProductId(format!("{}/{}", kind.product_prefix(), slug(&rel)));
        let mut requires: Vec<ProductId> = if kind.is_template() {
            parse_template(&String::from_utf8_lossy(&bytes))
                .iter()
                .filter_map(&product_of)
                .collect()
        } else {
            Vec::new()
        };
        requires.sort();
        requires.dedup();
        let meta = std::fs::metadata(&path).ok();
        let hash = format!("blake3:{}", blake3::hash(&bytes).to_hex());
        products.push(Product {
            canonical_id: id.clone(),
            vendor: "REAPER".into(),
            name: rel.clone(),
        });
        items.push(Item {
            kind: ItemKind::DawAsset,
            product_id: Some(id),
            format: None,
            platform: os,
            arch: None,
            version: None,
            path: PortablePath::logical(REAPER_RESOURCES, rel),
            content_hash: ContentHash::try_from(hash).ok(),
            size: Some(bytes.len() as u64),
            mtime: meta
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64),
            platform_portable: true,
            confidence: Confidence::High,
            requires,
        });
    }
    (products, items)
}

/// `reaper-kb.ini` text from another machine, with script paths under that
/// machine's resource folder (`from_dir`, spelled for `from_os`) moved under
/// this machine's (`to_dir`, for `to_os`). Paths elsewhere stay as they are.
pub fn remap_key_map(text: &str, from_dir: &str, from_os: Os, to_dir: &str, to_os: Os) -> String {
    let from = from_dir.trim_end_matches(['/', '\\']);
    if from.is_empty() {
        return text.to_owned();
    }
    let to = to_dir.trim_end_matches(['/', '\\']);
    let fold = |c: char| match c {
        '\\' => '/',
        c => c.to_ascii_lowercase(),
    };
    let needle: Vec<char> = from.chars().map(fold).collect();
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let hit = chars.len() - i >= needle.len()
            && chars[i..i + needle.len()]
                .iter()
                .map(|&c| fold(c))
                .eq(needle.iter().copied())
            && chars
                .get(i + needle.len())
                .is_some_and(|&c| c == '/' || c == '\\');
        if !hit {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        out.push_str(to);
        i += needle.len();
        // The rest of the path, up to its closing quote or the line's end.
        while i < chars.len() && !matches!(chars[i], '"' | '\r' | '\n') {
            let c = chars[i];
            out.push(if c == '/' || (c == '\\' && from_os == Os::Windows) {
                to_os.separator()
            } else {
                c
            });
            i += 1;
        }
    }
    out
}

/// Lowercase ASCII alphanumeric words joined by `-`.
pub(crate) fn slug(s: &str) -> String {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join("-")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_maps_and_menus_are_single_files() {
        assert_eq!(resource_kind("reaper-kb.ini"), Some(ResourceKind::KeyMap));
        assert_eq!(resource_kind("REAPER-MENU.INI"), Some(ResourceKind::Menus));
        assert_eq!(resource_kind("reaper.ini"), None);
        assert_eq!(
            resource_kind("Scripts/reaper-kb.ini"),
            Some(ResourceKind::Script)
        );
        assert!(ResourceKind::KeyMap.needs_reaper_closed());
        assert!(!ResourceKind::Script.needs_reaper_closed());
    }

    #[test]
    fn key_map_script_paths_follow_the_resource_folder() {
        let kb = "SCR 4 0 RS1 \"Custom: a.lua\" \"C:\\Users\\josh\\AppData\\Roaming\\REAPER\\Scripts\\Tools\\a.lua\"\r\n\
                  SCR 4 0 RS2 \"Custom: b.lua\" \"D:\\Other\\b.lua\"\r\n\
                  SCR 4 0 RS3 \"Custom: c.lua\" Tools/c.lua\r\n";
        let out = remap_key_map(
            kb,
            r"C:\Users\josh\AppData\Roaming\REAPER",
            Os::Windows,
            "/Users/josh/Library/Application Support/REAPER",
            Os::Macos,
        );
        assert!(out.contains(
            "\"/Users/josh/Library/Application Support/REAPER/Scripts/Tools/a.lua\"\r\n"
        ));
        assert!(out.contains(r#""D:\Other\b.lua""#));
        assert!(out.contains("Tools/c.lua\r\n"));
    }

    #[test]
    fn tokenizes_reaper_quoting() {
        assert_eq!(
            tokenize(r#"<VST "VST: A (B)" 'x y.dll' 0 "" 123<ab> `q"q`"#),
            ["<VST", "VST: A (B)", "x y.dll", "0", "", "123<ab>", "q\"q"]
        );
    }

    #[test]
    fn display_names() {
        assert_eq!(
            split_display("VST3i: Acme Synth (Acme Audio)"),
            Some((
                PluginFormat::Vst3,
                "Acme Synth".into(),
                Some("Acme Audio".into())
            ))
        );
        assert_eq!(
            split_display("VST: Acme EQ (Mono) (Acme Audio)"),
            Some((
                PluginFormat::Vst2,
                "Acme EQ (Mono)".into(),
                Some("Acme Audio".into())
            ))
        );
        assert_eq!(
            split_display("CLAP: Bare"),
            Some((PluginFormat::Clap, "Bare".into(), None))
        );
        assert_eq!(split_display("DX: Old"), None);
    }

    #[test]
    fn stock_plugins() {
        let refs = parse_template(concat!(
            "<VST \"VST: ReaEQ (Cockos)\" reaeq.dll 0 \"\" 1919247729<5653547265657170726561657100> \"\"\n",
            "<JS \"utility/volume\" \"\"\n",
            "<VST \"VST3: Acme Verb (Acme Audio)\" \"Acme Verb.vst3\" 0 \"\" 1{5653544163654D6163726F7665726200} \"\"\n",
        ));
        let stock: Vec<_> = refs
            .iter()
            .map(|r| (r.name.as_str(), r.is_stock()))
            .collect();
        assert_eq!(
            stock,
            [("volume", true), ("ReaEQ", true), ("Acme Verb", false)]
        );
    }

    #[test]
    fn vst3_without_class_id_falls_back_to_file_name() {
        let refs = parse_template(r#"<VST "VST3: Old (Acme)" "Old.vst3" 0 "" 12345 """#);
        assert_eq!(refs[0].identity_hint(), "vst3:Old.vst3");
    }

    #[test]
    fn resource_paths_from_other_machines_are_checked() {
        use ResourceKind::*;
        for (ok, kind) in [
            ("TrackTemplates/Vocals.RTrackTemplate", TrackTemplate),
            ("FXChains/Mix/Bus Glue.RfxChain", FxChain),
            ("ProjectTemplates/Podcast.rpp", ProjectTemplate),
            ("Scripts/ReaTeam Scripts/Items/X.lua", Script),
            ("Scripts/tool/icon.png", Script),
            ("ColorThemes/Imperial.ReaperThemeZip", Theme),
        ] {
            assert_eq!(resource_kind(ok), Some(kind), "{ok}");
        }
        for bad in [
            "reaper.ini",
            "FXChains/../reaper.ini",
            "FXChains/../../x.RfxChain",
            "/FXChains/a.RfxChain",
            "FXChains/a.RTrackTemplate",
            "FXChains/C:a.RfxChain",
            "FXChains\\a.RfxChain",
            "FXChains/a.RfxChain/",
            "FXChains//a.RfxChain",
            "Effects/a.RfxChain",
            "UserPlugins/reaper_sws64.dll",
            "Scripts/tool/helper.dll",
            "Scripts/.git/config",
            "Scripts/x/__pycache__/a.pyc",
            "Scripts",
        ] {
            assert_eq!(resource_kind(bad), None, "{bad}");
        }
    }

    #[test]
    fn resource_dirs() {
        assert_eq!(
            reaper_resource_dir(Os::Macos, "", "/Users/alice"),
            Some(PathBuf::from(
                "/Users/alice/Library/Application Support/REAPER"
            ))
        );
        assert!(reaper_resource_dir(Os::Windows, "", "/Users/alice").is_none());
    }
}
