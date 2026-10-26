//! Which of Reaper's files only work together (decision #62): a script and
//! the files it loads, a JSFX and its imports, a theme and its image folder,
//! every file of one ReaPack package. Found at scan time on the machine that
//! has the files and stored as [`Item::needs_files`], so another machine can
//! bring a script over with everything it needs.
//!
//! [`Item::needs_files`]: patchpilot_core::Item::needs_files

use std::collections::{BTreeMap, BTreeSet, HashMap};

use patchpilot_core::{Manifest, PortablePath, ProductId};

use crate::reaper::REAPER_RESOURCES;

/// The tracked files under the resource folder, to resolve references against.
pub struct Known {
    /// Lowercased path → path as found (NTFS and APFS ignore case).
    by_lower: HashMap<String, String>,
    /// Lowercased file name → paths with that name.
    by_name: HashMap<String, Vec<String>>,
}

impl Known {
    pub fn new<'a>(rels: impl IntoIterator<Item = &'a str>) -> Self {
        let mut by_lower = HashMap::new();
        let mut by_name: HashMap<String, Vec<String>> = HashMap::new();
        for rel in rels {
            let lower = rel.to_lowercase();
            let name = lower.rsplit('/').next().unwrap_or(&lower).to_owned();
            by_name.entry(name).or_default().push(rel.to_owned());
            by_lower.insert(lower, rel.to_owned());
        }
        Self { by_lower, by_name }
    }

    /// The tracked path spelled like `rel`, ignoring case.
    pub fn get(&self, rel: &str) -> Option<&str> {
        self.by_lower.get(&rel.to_lowercase()).map(String::as_str)
    }
}

/// Files `rel` (a tracked file with contents `bytes`) needs beside it, as
/// tracked paths, without itself. Only text formats are read: Lua (`dofile`,
/// `require` and file paths in strings), EEL, JSFX (`import`, `filename:`)
/// and `.ReaperTheme` (its image folder).
pub fn loaded_files(rel: &str, bytes: &[u8], known: &Known) -> Vec<String> {
    let lower = rel.to_lowercase();
    let ext = lower
        .rsplit('/')
        .next()
        .and_then(|n| n.rsplit_once('.'))
        .map(|(_, e)| e);
    let in_effects = lower.starts_with("effects/");
    let text = || String::from_utf8_lossy(bytes);
    let mut out = BTreeSet::new();
    match ext {
        Some("lua") | Some("eel") => {
            let text = text();
            for line in text.lines() {
                let loads = ["dofile", "loadfile", "require", "import"]
                    .iter()
                    .any(|w| line.contains(w));
                let requires = line.contains("require");
                for lit in literals(line, ext == Some("lua")) {
                    if let Some(found) = resolve(rel, &lit, known, &[], loads) {
                        out.insert(found);
                    } else if requires && !lit.contains(['/', '\\']) {
                        // require "lib.json" → lib/json.lua
                        let module = format!("{}.lua", lit.replace('.', "/"));
                        if let Some(found) = resolve(rel, &module, known, &[], false) {
                            out.insert(found);
                        }
                    }
                }
            }
        }
        Some("jsfx") | Some("jsfx-inc") | None if in_effects || ext.is_some() => {
            let text = text();
            for line in text.lines() {
                let line = line.trim();
                if let Some(path) = line.strip_prefix("import ") {
                    let found = resolve(rel, path.trim(), known, &["Effects"], true);
                    out.extend(found);
                } else if let Some(spec) = line.strip_prefix("filename:") {
                    let path = spec.split_once(',').map_or("", |(_, p)| p.trim());
                    let found = resolve(rel, path, known, &["Data", "Effects"], false);
                    out.extend(found);
                }
            }
        }
        Some("reapertheme") => {
            // `ui_img=Name` names the folder with the theme's images; a
            // folder named like the theme is used when it's missing.
            let text = text();
            let dir = parent(rel);
            let stem = rel.rsplit('/').next().unwrap_or(rel);
            let stem = stem.rsplit_once('.').map_or(stem, |(s, _)| s);
            let mut folders = vec![format!("{dir}/{stem}/").to_lowercase()];
            for line in text.lines() {
                if let Some(v) = line.trim().strip_prefix("ui_img=") {
                    let v = v.trim().replace('\\', "/");
                    if !v.is_empty() && !v.contains("..") {
                        folders.push(format!("{dir}/{}/", v.trim_end_matches('/')).to_lowercase());
                    }
                }
            }
            for (lower, path) in &known.by_lower {
                if folders.iter().any(|f| lower.starts_with(f.as_str())) {
                    out.insert(path.clone());
                }
            }
        }
        _ => {}
    }
    out.remove(rel);
    out.into_iter().collect()
}

/// String literals on one line: `"..."`, `'...'` and Lua's `[[...]]`. Lua
/// comments (`--`) end the line.
fn literals(line: &str, lua: bool) -> Vec<String> {
    let chars: Vec<char> = line.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '-' if lua && chars.get(i + 1) == Some(&'-') => break,
            q @ ('"' | '\'') => {
                let mut s = String::new();
                i += 1;
                while i < chars.len() && chars[i] != q {
                    if chars[i] == '\\' && i + 1 < chars.len() {
                        i += 1;
                    }
                    s.push(chars[i]);
                    i += 1;
                }
                out.push(s);
            }
            '[' if lua && chars.get(i + 1) == Some(&'[') => {
                let rest: String = chars[i + 2..].iter().collect();
                let (s, _) = rest.split_once("]]").unwrap_or((&rest, ""));
                i += 2 + s.chars().count() + 1;
                out.push(s.to_owned());
            }
            _ => {}
        }
        i += 1;
    }
    out.retain(|s| plausible_path(s));
    out
}

/// Looks like a file path: has a name with an extension, no URL or pattern.
fn plausible_path(s: &str) -> bool {
    let name = s.rsplit(['/', '\\']).next().unwrap_or(s);
    (3..=260).contains(&s.len())
        && name.contains('.')
        && !name.ends_with('.')
        && !s.contains("://")
        && !s.contains(['%', '*', '?', '\n', '|', '<', '>'])
}

/// The tracked file a reference in `rel` points to: next to `rel`, from the
/// resource folder, below one of `roots`, by its path's tail, or (`by_name`)
/// a file of that name in the same top folder (`Scripts/MPL Scripts`).
fn resolve(rel: &str, path: &str, known: &Known, roots: &[&str], by_name: bool) -> Option<String> {
    let path = path.replace('\\', "/");
    let path = path.trim().trim_start_matches('/');
    if path.is_empty() {
        return None;
    }
    let mut tries = vec![join(parent(rel), path), normalize(path)];
    tries.extend(roots.iter().map(|r| join(r, path)));
    if let Some(found) = tries.into_iter().flatten().find_map(|p| known.get(&p)) {
        return Some(found.to_owned());
    }
    let tail = normalize(path)?.to_lowercase();
    let top = top_folder(rel).to_lowercase();
    if tail.contains('/') {
        let suffix = format!("/{tail}");
        let mut hits: Vec<&String> = known
            .by_lower
            .iter()
            .filter(|(l, _)| l.ends_with(&suffix))
            .map(|(_, p)| p)
            .collect();
        hits.sort();
        let near: Vec<&String> = hits
            .iter()
            .copied()
            .filter(|p| p.to_lowercase().starts_with(&top))
            .collect();
        return match (near.as_slice(), hits.as_slice()) {
            ([one], _) | ([], [one]) => Some((*one).clone()),
            _ => None,
        };
    }
    if !by_name {
        return None;
    }
    let near: Vec<&String> = known
        .by_name
        .get(&tail)?
        .iter()
        .filter(|p| p.to_lowercase().starts_with(&top))
        .collect();
    match near.as_slice() {
        [one] => Some((*one).clone()),
        _ => None,
    }
}

/// `Scripts/MPL Scripts/Items/x.lua` → `Scripts/MPL Scripts/`.
fn top_folder(rel: &str) -> String {
    let parts: Vec<&str> = rel.split('/').collect();
    match parts.as_slice() {
        [a, b, _, ..] => format!("{a}/{b}/"),
        [a, ..] => format!("{a}/"),
        [] => String::new(),
    }
}

fn parent(rel: &str) -> &str {
    rel.rsplit_once('/').map_or("", |(d, _)| d)
}

/// `dir` + `path` with `.` and `..` resolved; `None` if it leaves the root.
fn join(dir: &str, path: &str) -> Option<String> {
    if dir.is_empty() {
        normalize(path)
    } else {
        normalize(&format!("{dir}/{path}"))
    }
}

fn normalize(path: &str) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    for p in path.split('/') {
        match p {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            p => parts.push(p),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// Ties each group's files to each other (a ReaPack package): the first
/// tracked file needs the rest and each of the rest needs the first, so
/// following needs from any of them reaches all, without listing every pair.
pub fn link_groups(
    needs: &mut BTreeMap<String, BTreeSet<String>>,
    groups: &[Vec<String>],
    known: &Known,
) {
    for group in groups {
        let files: Vec<&str> = group
            .iter()
            .filter_map(|f| known.get(&f.replace('\\', "/")))
            .collect();
        let Some((hub, rest)) = files.split_first() else {
            continue;
        };
        for f in rest {
            if f == hub {
                continue;
            }
            needs
                .entry((*hub).to_owned())
                .or_default()
                .insert((*f).to_owned());
            needs
                .entry((*f).to_owned())
                .or_default()
                .insert((*hub).to_owned());
        }
    }
}

/// `start` and every file it needs, directly or through others, by following
/// `needs` (paths compare ignoring case). Sorted, as `needs` spells them.
pub fn with_needs<'a>(
    start: impl IntoIterator<Item = &'a str>,
    needs: impl Fn(&str) -> Option<&'a [String]>,
) -> BTreeSet<String> {
    let mut seen: BTreeMap<String, String> = BTreeMap::new();
    let mut todo: Vec<&str> = start.into_iter().collect();
    while let Some(f) = todo.pop() {
        if seen.insert(f.to_lowercase(), f.to_owned()).is_some() {
            continue;
        }
        if let Some(next) = needs(f) {
            todo.extend(next.iter().map(String::as_str));
        }
    }
    seen.into_values().collect()
}

/// Every Reaper resource file in `m` (by product) mapped to the unit it
/// belongs to, named by the unit's first product: files that need each other
/// both ways (a ReaPack package, a theme and its images) are one unit. A
/// library many scripts load stays its own unit. Unlinked files map to
/// themselves.
pub fn units(m: &Manifest) -> BTreeMap<ProductId, ProductId> {
    let files: Vec<(&ProductId, &str, &[String])> = m
        .items
        .iter()
        .filter_map(|i| match (&i.product_id, &i.path) {
            (Some(id), PortablePath::Logical { root, rel }) if root == REAPER_RESOURCES => {
                Some((id, rel.as_str(), i.needs_files.as_slice()))
            }
            _ => None,
        })
        .collect();
    let index: HashMap<String, usize> = files
        .iter()
        .enumerate()
        .map(|(n, (_, rel, _))| (rel.to_lowercase(), n))
        .collect();
    let needs = |a: usize, b: usize| {
        files[a]
            .2
            .iter()
            .any(|f| f.eq_ignore_ascii_case(files[b].1))
    };
    let mut parent: Vec<usize> = (0..files.len()).collect();
    fn root(parent: &mut [usize], mut n: usize) -> usize {
        while parent[n] != n {
            parent[n] = parent[parent[n]];
            n = parent[n];
        }
        n
    }
    for (a, (_, _, needs_files)) in files.iter().enumerate() {
        for f in *needs_files {
            let Some(&b) = index.get(&f.to_lowercase()) else {
                continue;
            };
            if needs(b, a) {
                let (ra, rb) = (root(&mut parent, a), root(&mut parent, b));
                parent[ra.max(rb)] = ra.min(rb);
            }
        }
    }
    let mut first: BTreeMap<usize, &ProductId> = BTreeMap::new();
    for (n, (id, _, _)) in files.iter().enumerate() {
        let r = root(&mut parent, n);
        let first = first.entry(r).or_insert(id);
        if *id < *first {
            *first = id;
        }
    }
    (0..files.len())
        .map(|n| {
            let r = root(&mut parent, n);
            (files[n].0.clone(), first[&r].clone())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn known(rels: &[&str]) -> Known {
        Known::new(rels.iter().copied())
    }

    #[test]
    fn lua_scripts_need_the_files_they_load() {
        let k = known(&[
            "Scripts/MPL Scripts/Items/mpl_Split.lua",
            "Scripts/MPL Scripts/Functions/mpl_Various_functions.lua",
            "Scripts/Tool/main.lua",
            "Scripts/Tool/lib/gui.lua",
            "Scripts/Tool/lib/json.lua",
            "Scripts/Tool/img/logo.png",
            "Scripts/Other/lib/gui.lua",
        ]);
        let script = br#"
local info = debug.getinfo(1,'S')
local script_path = info.source:match([[^@?(.*[\/])[^\/]-$]])
dofile(script_path .. "lib/gui.lua")
local json = require("lib.json")
local img = script_path .. 'img/logo.png'
-- dofile(script_path .. "lib/old.lua")
"#;
        assert_eq!(
            loaded_files("Scripts/Tool/main.lua", script, &k),
            [
                "Scripts/Tool/img/logo.png",
                "Scripts/Tool/lib/gui.lua",
                "Scripts/Tool/lib/json.lua"
            ]
        );
        // From the resource folder, and by name in the same repository folder.
        let mpl = br#"dofile(reaper.GetResourcePath()..'/Scripts/MPL Scripts/Functions/mpl_Various_functions.lua')"#;
        assert_eq!(
            loaded_files("Scripts/MPL Scripts/Items/mpl_Split.lua", mpl, &k),
            ["Scripts/MPL Scripts/Functions/mpl_Various_functions.lua"]
        );
        let by_name = br#"dofile(VF_path..'/mpl_various_functions.lua')"#;
        assert_eq!(
            loaded_files("Scripts/MPL Scripts/Items/mpl_Split.lua", by_name, &k),
            ["Scripts/MPL Scripts/Functions/mpl_Various_functions.lua"]
        );
        // A bare name in a plain string isn't enough, nor an ambiguous tail.
        let loose = br#"local n = "gui.lua"; local m = "lib/gui.lua""#;
        assert!(loaded_files("Scripts/Third/x.lua", loose, &k).is_empty());
        // Binary files aren't read.
        assert!(loaded_files("Scripts/Tool/img/logo.png", b"\"lib/gui.lua\"", &k).is_empty());
    }

    #[test]
    fn jsfx_need_their_imports_and_data() {
        let k = known(&[
            "Effects/ReaTeam JSFX/Dynamics/comp.jsfx",
            "Effects/ReaTeam JSFX/Dynamics/comp.jsfx-inc",
            "Effects/cookdsp/cookdsp.jsfx-inc",
            "Data/amp_models/clean.wav",
        ]);
        let jsfx = b"desc:Comp\nimport comp.jsfx-inc\nimport cookdsp/cookdsp.jsfx-inc\nfilename:0,amp_models/clean.wav\n";
        assert_eq!(
            loaded_files("Effects/ReaTeam JSFX/Dynamics/comp.jsfx", jsfx, &k),
            [
                "Data/amp_models/clean.wav",
                "Effects/ReaTeam JSFX/Dynamics/comp.jsfx-inc",
                "Effects/cookdsp/cookdsp.jsfx-inc",
            ]
        );
    }

    #[test]
    fn themes_need_their_image_folder() {
        let k = known(&[
            "ColorThemes/Flat.ReaperTheme",
            "ColorThemes/Flat/track_panel.png",
            "ColorThemes/Flat/rtconfig.txt",
            "ColorThemes/Imperial.ReaperThemeZip",
            "ColorThemes/Shared/knob.png",
        ]);
        assert_eq!(
            loaded_files("ColorThemes/Flat.ReaperTheme", b"[color theme]\n", &k),
            [
                "ColorThemes/Flat/rtconfig.txt",
                "ColorThemes/Flat/track_panel.png"
            ]
        );
        assert_eq!(
            loaded_files(
                "ColorThemes/Flat.ReaperTheme",
                b"[REAPER]\nui_img=Shared\n",
                &k
            )
            .len(),
            3
        );
    }

    #[test]
    fn package_files_reach_each_other() {
        let k = known(&["Scripts/R/a.lua", "Scripts/R/b.lua", "Effects/R/c.jsfx"]);
        let mut needs = BTreeMap::new();
        link_groups(
            &mut needs,
            &[vec![
                "Scripts/R/a.lua".into(),
                "Scripts\\R\\b.lua".into(),
                "effects/r/c.jsfx".into(),
                "Scripts/R/gone.lua".into(),
            ]],
            &k,
        );
        let needs: BTreeMap<String, Vec<String>> = needs
            .into_iter()
            .map(|(k, v)| (k, v.into_iter().collect()))
            .collect();
        let all = with_needs(["Effects/R/c.jsfx"], |f| needs.get(f).map(Vec::as_slice));
        assert_eq!(all.len(), 3, "{all:?}");
    }

    #[test]
    fn units_join_packages_but_not_shared_libraries() {
        use patchpilot_core::{Arch, Confidence, Item, ItemKind, MachineInfo, Os, PathMap};
        let file = |rel: &str, needs: &[&str]| Item {
            kind: ItemKind::DawAsset,
            product_id: Some(ProductId(format!("reaper-script/{rel}"))),
            format: None,
            platform: Os::Macos,
            arch: None,
            version: None,
            path: PortablePath::logical(REAPER_RESOURCES, rel),
            content_hash: None,
            size: None,
            mtime: None,
            platform_portable: true,
            confidence: Confidence::High,
            requires: Vec::new(),
            needs_files: needs.iter().map(|s| s.to_string()).collect(),
        };
        let mut m = Manifest::new(
            MachineInfo {
                id: "m".into(),
                name: "M".into(),
                os: Os::Macos,
                arch: Arch::Aarch64,
                os_version: None,
                path_map: PathMap::new(Os::Macos),
            },
            0,
        );
        m.items = vec![
            file("p/a", &["p/b", "p/c"]),
            file("p/b", &["p/a"]),
            file("p/c", &["p/a", "lib"]),
            file("x", &["lib"]),
            file("lib", &[]),
        ];
        let u = units(&m);
        let unit = |rel: &str| u[&ProductId(format!("reaper-script/{rel}"))].0.clone();
        assert_eq!(unit("p/c"), "reaper-script/p/a");
        assert_eq!(unit("p/b"), "reaper-script/p/a");
        assert_eq!(unit("x"), "reaper-script/x");
        assert_eq!(unit("lib"), "reaper-script/lib");
    }
}
