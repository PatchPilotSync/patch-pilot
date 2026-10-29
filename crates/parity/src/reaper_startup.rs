//! Reaper's startup action (decision #107): the action the SWS extension runs
//! every time Reaper opens, set with Extensions > Startup actions > Set global
//! startup action and kept in the resource folder's `S&M.ini` as
//! `[Misc] GlobalStartupAction=<id>`. A custom action that starts a rig's
//! scripts (nvk Folder Items, say) is easy to set once and forget, so the
//! Primary Rig's is compared, brought over and kept in sync like a settings
//! file, but only that one line of `S&M.ini`: the rest holds SWS's window
//! states and this machine's paths.
//!
//! The id is what SWS keeps: a native action's number (`40044`), or a named
//! one (`_SWS_ABOUT`, a custom action's `_<32 hex>`, a script's `_RS<hex>`).
//! Custom actions and scripts are defined in `reaper-kb.ini`, which already
//! syncs, so the same id means the same action on every machine that has it.

use std::collections::{BTreeSet, HashMap};

use patchpilot_core::ProductId;
use serde::Serialize;

use crate::reaper::{KEY_MAP_ROOT, tokenize};

/// SWS's settings file at the top of Reaper's resource folder.
pub const SWS_INI: &str = "S&M.ini";
const SECTION: &str = "Misc";
const KEY: &str = "GlobalStartupAction";

/// The script Reaper itself runs at startup (`Scripts/__startup.lua`), the
/// other way to run something when Reaper opens. It is a script, so it
/// syncs as one.
pub const STARTUP_SCRIPT: &str = "Scripts/__startup.lua";

/// The global startup action in `S&M.ini` text, if one is set.
pub fn global_action(sws_ini: &str) -> Option<&str> {
    let id = crate::ini::get(sws_ini, SECTION, KEY)?;
    let id = id.trim_matches('"').trim();
    (!id.is_empty()).then_some(id)
}

/// The part of `S&M.ini` other machines get: just the startup action, or
/// nothing when none is set (or the file isn't text).
pub fn view(sws_ini: &[u8]) -> Vec<u8> {
    std::str::from_utf8(sws_ini)
        .ok()
        .and_then(global_action)
        .map(view_for)
        .unwrap_or_default()
}

/// The shared part of an `S&M.ini` whose startup action is `id`.
pub fn view_for(id: &str) -> Vec<u8> {
    format!("[{SECTION}]\n{KEY}={id}\n").into_bytes()
}

/// This machine's `S&M.ini` (`current`, `None` when there is none yet) with
/// the startup action of `incoming` (another machine's shared part) set in
/// it, every other line kept as it was. A shared part without one changes
/// nothing: a startup action is never cleared from another machine.
pub fn merge(current: Option<&[u8]>, incoming: &[u8]) -> Result<Vec<u8>, String> {
    let current = current.unwrap_or_default();
    let Some(id) = std::str::from_utf8(incoming).ok().and_then(global_action) else {
        return Ok(current.to_vec());
    };
    let text = std::str::from_utf8(current)
        .map_err(|_| format!("{SWS_INI} here isn't text Patch Pilot can change safely"))?;
    if global_action(text) == Some(id) {
        return Ok(current.to_vec());
    }
    Ok(crate::ini::set(text, SECTION, &[(KEY.to_owned(), id.to_owned())]).into_bytes())
}

/// What a startup action is. Mirrored in `app/src/api.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    /// One of Reaper's own actions, by number.
    Native,
    /// A custom action from the key map (`ACT`).
    Custom,
    /// A ReaScript from the key map (`SCR`).
    Script,
    /// An extension's action (`_SWS_…`, `_BR_…`): there when the extension is.
    Extension,
    /// Looks like a custom action or script, but this key map doesn't have it,
    /// so SWS can't run it here.
    Missing,
}

/// A startup action, as one machine's key map describes it. Mirrored in
/// `app/src/api.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StartupAction {
    /// As SWS keeps it: `_8f3c…` or `40044`.
    pub id: String,
    /// As the Action list shows it: "Custom: JMD_Startup".
    pub name: Option<String>,
    pub kind: ActionKind,
    /// The scripts it runs, itself or through the custom actions in it, as
    /// `/`-separated paths below the resource folder (`Scripts/…`). Scripts
    /// outside the resource folder are left out.
    pub scripts: Vec<String>,
}

/// One custom action or script in a key map.
struct Defined {
    name: String,
    /// A custom action's steps, as command ids.
    steps: Vec<String>,
    /// A script's path as the key map spells it.
    script: Option<String>,
}

/// Main-section custom actions and scripts in key map text, by id without
/// its `_`, lowercased (custom action ids are hex, matched either case).
fn defined(key_map: &str) -> HashMap<String, Defined> {
    let mut out = HashMap::new();
    for line in key_map.lines() {
        let t = tokenize(line);
        let [kind, _flags, section, id, name, rest @ ..] = &t[..] else {
            continue;
        };
        // Section 0 is the Main section, the only one SWS runs at startup.
        if section != "0" {
            continue;
        }
        let def = match kind.as_str() {
            "ACT" => Defined {
                name: name.clone(),
                steps: rest.to_vec(),
                script: None,
            },
            "SCR" => Defined {
                name: name.clone(),
                steps: Vec::new(),
                script: rest.first().cloned(),
            },
            _ => continue,
        };
        out.insert(id.trim_start_matches('_').to_lowercase(), def);
    }
    out
}

/// A key map's script path as a path below the resource folder: relative
/// paths are under `Scripts/`, and a portable key map spells this machine's
/// resource folder [`KEY_MAP_ROOT`]. `None` for scripts elsewhere.
fn script_rel(path: &str) -> Option<String> {
    let p = path.replace('\\', "/");
    let root = format!("{KEY_MAP_ROOT}/");
    if p.len() > root.len() && p[..root.len()].eq_ignore_ascii_case(&root) {
        return Some(p[root.len()..].to_owned());
    }
    let absolute = p.starts_with('/') || p.as_bytes().get(1) == Some(&b':') || p.starts_with('<');
    (!absolute && !p.is_empty()).then(|| format!("Scripts/{p}"))
}

/// Whether a named id looks like one the key map would define: a custom
/// action's 32 hex digits, or a script's `RS` and hex.
fn looks_defined(bare: &str) -> bool {
    let hex = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_hexdigit());
    (bare.len() == 32 && hex(bare))
        || bare
            .get(..2)
            .is_some_and(|p| p.eq_ignore_ascii_case("rs") && hex(&bare[2..]))
}

/// What `id` is, read from `key_map` (a portable key map: see
/// [`crate::reaper::portable_key_map`]).
pub fn describe(id: &str, key_map: &str) -> StartupAction {
    let defs = defined(key_map);
    let bare = id.trim_start_matches('_');
    let (name, kind) = if !id.starts_with('_') && id.chars().all(|c| c.is_ascii_digit()) {
        (None, ActionKind::Native)
    } else {
        match defs.get(&bare.to_lowercase()) {
            Some(d) if d.script.is_some() => (Some(d.name.clone()), ActionKind::Script),
            Some(d) => (Some(d.name.clone()), ActionKind::Custom),
            None if looks_defined(bare) => (None, ActionKind::Missing),
            None => (None, ActionKind::Extension),
        }
    };
    // The scripts it runs, through nested custom actions too.
    let mut scripts = BTreeSet::new();
    let mut seen = BTreeSet::new();
    let mut todo = vec![bare.to_lowercase()];
    while let Some(next) = todo.pop() {
        if !seen.insert(next.clone()) {
            continue;
        }
        let Some(d) = defs.get(&next) else {
            continue;
        };
        if let Some(rel) = d.script.as_deref().and_then(script_rel) {
            scripts.insert(rel);
        }
        todo.extend(
            d.steps
                .iter()
                .filter_map(|s| s.strip_prefix('_'))
                .map(str::to_lowercase),
        );
    }
    StartupAction {
        id: id.to_owned(),
        name,
        kind,
        scripts: scripts.into_iter().collect(),
    }
}

/// Every id `key_map` defines, as SWS would keep it (`_<id>`): the startup
/// actions a machine with this key map could have set.
pub fn defined_ids(key_map: &str) -> Vec<String> {
    let mut ids: Vec<String> = key_map
        .lines()
        .filter_map(|line| {
            let t = tokenize(line);
            match &t[..] {
                [kind, _, section, id, ..]
                    if (kind == "ACT" || kind == "SCR") && section == "0" =>
                {
                    Some(format!("_{}", id.trim_start_matches('_')))
                }
                _ => None,
            }
        })
        .collect();
    ids.sort();
    ids.dedup();
    ids
}

/// The extension that runs startup actions: SWS.
pub fn needs() -> ProductId {
    crate::reaper_ext::product_id("sws")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SWS: &str = "[General]\r\nIniFileUpgrade=9\r\n\r\n[Misc]\r\nRemoveAllEnvsSelTracksPrompt=1\r\nGlobalStartupAction=_8f3c1d2e4b5a69788796a5b4c3d2e1f0\r\n\r\n[RESOURCE_VIEW]\r\nFilter=1\r\n";

    const KB: &str = "KEY 1 65 _8f3c1d2e4b5a69788796a5b4c3d2e1f0 0\r\n\
        ACT 0 0 \"8f3c1d2e4b5a69788796a5b4c3d2e1f0\" \"Custom: JMD_Startup\" _RSaa11 _0123456789abcdef0123456789abcdef 40044\r\n\
        ACT 0 0 \"0123456789abcdef0123456789abcdef\" \"Custom: Inner\" _RSbb22 _SWS_ABOUT\r\n\
        SCR 4 0 RSaa11 \"Custom: nvk_FOLDER_ITEMS.lua\" \"nvk_FOLDER_ITEMS/nvk_FOLDER_ITEMS.lua\"\r\n\
        SCR 4 0 RSbb22 \"Custom: tool.lua\" \"<REAPER_RESOURCES>/Scripts/Tools/tool.lua\"\r\n\
        SCR 4 0 RScc33 \"Custom: elsewhere.lua\" \"D:/Elsewhere/x.lua\"\r\n\
        SCR 4 32060 RSdd44 \"Custom: midi.lua\" \"midi.lua\"\r\n";

    #[test]
    fn reads_the_global_startup_action() {
        assert_eq!(
            global_action(SWS),
            Some("_8f3c1d2e4b5a69788796a5b4c3d2e1f0")
        );
        assert_eq!(global_action("[Misc]\nGlobalStartupAction=\n"), None);
        assert_eq!(global_action("[General]\nGlobalStartupAction=_x\n"), None);
        assert_eq!(global_action(""), None);
    }

    #[test]
    fn shares_only_the_startup_action() {
        assert_eq!(
            view(SWS.as_bytes()),
            b"[Misc]\nGlobalStartupAction=_8f3c1d2e4b5a69788796a5b4c3d2e1f0\n"
        );
        assert!(view(b"[General]\nBeta=0\n").is_empty());
        assert!(view(&[0xff, 0xfe, 0x00]).is_empty());
    }

    #[test]
    fn merges_the_startup_action_into_this_machines_file() {
        let incoming = view_for("_abc");
        // Replaced in place, everything else as it was, CRLF kept.
        let out = String::from_utf8(merge(Some(SWS.as_bytes()), &incoming).unwrap()).unwrap();
        assert_eq!(
            out,
            SWS.replace("_8f3c1d2e4b5a69788796a5b4c3d2e1f0", "_abc")
        );
        // Added to a file without one.
        let out = merge(Some(b"[General]\nBeta=0\n"), &incoming).unwrap();
        assert_eq!(
            out,
            b"[General]\nBeta=0\n\n[Misc]\nGlobalStartupAction=_abc\n"
        );
        // And to a machine with no S&M.ini yet.
        assert_eq!(merge(None, &incoming).unwrap(), incoming);
        // The same action, or none coming, changes nothing.
        let same = view_for("_8f3c1d2e4b5a69788796a5b4c3d2e1f0");
        assert_eq!(merge(Some(SWS.as_bytes()), &same).unwrap(), SWS.as_bytes());
        assert_eq!(merge(Some(SWS.as_bytes()), b"").unwrap(), SWS.as_bytes());
        // A file that isn't text is left alone.
        assert!(merge(Some(&[0xff, 0xfe]), &incoming).is_err());
    }

    #[test]
    fn describes_a_custom_action_and_the_scripts_it_runs() {
        let a = describe("_8f3c1d2e4b5a69788796a5b4c3d2e1f0", KB);
        assert_eq!(a.kind, ActionKind::Custom);
        assert_eq!(a.name.as_deref(), Some("Custom: JMD_Startup"));
        assert_eq!(
            a.scripts,
            [
                "Scripts/Tools/tool.lua",
                "Scripts/nvk_FOLDER_ITEMS/nvk_FOLDER_ITEMS.lua"
            ]
        );
        // Ids match either case, as SWS's lookup does.
        let a = describe("_8F3C1D2E4B5A69788796A5B4C3D2E1F0", KB);
        assert_eq!(a.kind, ActionKind::Custom);
    }

    #[test]
    fn tells_scripts_native_extension_and_missing_actions_apart() {
        let s = describe("_RSaa11", KB);
        assert_eq!(s.kind, ActionKind::Script);
        assert_eq!(s.scripts, ["Scripts/nvk_FOLDER_ITEMS/nvk_FOLDER_ITEMS.lua"]);
        // Scripts outside the resource folder aren't files Patch Pilot tracks.
        assert!(describe("_RScc33", KB).scripts.is_empty());
        assert_eq!(describe("40044", KB).kind, ActionKind::Native);
        assert_eq!(describe("_SWS_ABOUT", KB).kind, ActionKind::Extension);
        assert_eq!(
            describe("_ffffffffffffffffffffffffffffffff", KB).kind,
            ActionKind::Missing
        );
        assert_eq!(describe("_RS9999", KB).kind, ActionKind::Missing);
        // Only the Main section: a MIDI editor script isn't a startup action.
        assert_eq!(describe("_RSdd44", KB).kind, ActionKind::Missing);
    }

    #[test]
    fn lists_the_actions_a_key_map_defines() {
        assert_eq!(
            defined_ids(KB),
            [
                "_0123456789abcdef0123456789abcdef",
                "_8f3c1d2e4b5a69788796a5b4c3d2e1f0",
                "_RSaa11",
                "_RSbb22",
                "_RScc33"
            ]
        );
    }
}
