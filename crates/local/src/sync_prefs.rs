//! Sync preferences (decision #80): which kinds of files and settings
//! keeping files in sync carries for this machine, each on unless turned
//! off. A kind turned off here neither comes here from linked machines nor
//! goes from here to them (backups say which kinds their machine keeps to
//! itself), so each machine can keep its own. Backups and Sync with still
//! hold everything. Kept in app-data `sync-prefs.json`, per machine.

use std::collections::BTreeSet;
use std::path::Path;

use patchpilot_parity::ableton::{
    ABLETON_PREFS, ABLETON_USER_LIBRARY, LIBRARY_CFG, prefs_label, user_library_label,
};
use patchpilot_parity::presets::is_preset_root;
use patchpilot_parity::reaper::{REAPER_RESOURCES, ResourceKind, resource_kind};
use serde::{Deserialize, Serialize};

const PREFS_FILE: &str = "sync-prefs.json";

/// A kind of file or setting that can be turned off. Mirrored in `app/src/api.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SyncKind {
    pub id: &'static str,
    /// The section it's listed under: "Reaper", "Ableton Live" or "Plug-ins".
    pub app: &'static str,
    pub label: &'static str,
}

const fn kind(id: &'static str, app: &'static str, label: &'static str) -> SyncKind {
    SyncKind { id, app, label }
}

/// Live's Collection names and which Collections and categories are shown,
/// from `Library.cfg`.
pub const COLLECTIONS: &str = "ableton/collections";
/// Live's Places, from `Library.cfg`.
pub const PLACES: &str = "ableton/places";

/// Every kind, in the order the list shows them.
pub const KINDS: &[SyncKind] = &[
    kind("reaper/track-templates", "Reaper", "Track templates"),
    kind("reaper/project-templates", "Reaper", "Project templates"),
    kind("reaper/fx-chains", "Reaper", "FX chains"),
    kind("reaper/fx-presets", "Reaper", "FX presets"),
    kind("reaper/scripts", "Reaper", "Scripts, JSFX and their data"),
    kind("reaper/themes", "Reaper", "Themes"),
    kind("reaper/theme-settings", "Reaper", "Theme adjuster settings"),
    kind("reaper/toolbar-icons", "Reaper", "Toolbar icons"),
    kind("reaper/key-map", "Reaper", "Shortcuts and custom actions"),
    kind("reaper/menus", "Reaper", "Menus and toolbars"),
    kind("reaper/mouse", "Reaper", "Mouse modifiers"),
    kind("ableton/templates", "Ableton Live", "Templates"),
    kind("ableton/defaults", "Ableton Live", "Default presets"),
    kind("ableton/presets", "Ableton Live", "Racks and presets"),
    kind("ableton/m4l", "Ableton Live", "Max for Live devices"),
    kind("ableton/clips", "Ableton Live", "Clips"),
    kind("ableton/grooves", "Ableton Live", "Grooves"),
    kind("ableton/tunings", "Ableton Live", "Tunings"),
    kind(
        COLLECTIONS,
        "Ableton Live",
        "Collection names and what's shown",
    ),
    kind("ableton/tags", "Ableton Live", "What's in each Collection"),
    kind(PLACES, "Ableton Live", "Places"),
    kind(
        "ableton/remote-scripts",
        "Ableton Live",
        "Control surface scripts",
    ),
    kind("ableton/options", "Ableton Live", "Options.txt"),
    kind("presets", "Plug-ins", "Plug-in presets"),
];

/// The kind of a file below `root`, if one can be turned off. `Library.cfg`
/// has none of its own: its parts are [`COLLECTIONS`] and [`PLACES`].
pub fn kind_of(root: &str, rel: &str) -> Option<&'static str> {
    if is_preset_root(root) {
        return Some("presets");
    }
    Some(match root {
        REAPER_RESOURCES => match resource_kind(rel)? {
            ResourceKind::TrackTemplate => "reaper/track-templates",
            ResourceKind::ProjectTemplate => "reaper/project-templates",
            ResourceKind::FxChain => "reaper/fx-chains",
            ResourceKind::FxPreset => "reaper/fx-presets",
            ResourceKind::Script | ResourceKind::Effect | ResourceKind::ScriptData => {
                "reaper/scripts"
            }
            ResourceKind::Theme => "reaper/themes",
            ResourceKind::ThemeSettings => "reaper/theme-settings",
            ResourceKind::ToolbarIcon => "reaper/toolbar-icons",
            ResourceKind::KeyMap => "reaper/key-map",
            ResourceKind::Menus => "reaper/menus",
            ResourceKind::MouseModifiers => "reaper/mouse",
        },
        ABLETON_USER_LIBRARY => match user_library_label(rel)? {
            "Template" => "ableton/templates",
            "Default preset" => "ableton/defaults",
            "Rack" | "Preset" => "ableton/presets",
            "Max for Live device" => "ableton/m4l",
            "Clip" => "ableton/clips",
            "Groove" => "ableton/grooves",
            "Tuning" => "ableton/tunings",
            "Collections" => "ableton/tags",
            "Control surface script" => "ableton/remote-scripts",
            _ => return None,
        },
        ABLETON_PREFS if rel.eq_ignore_ascii_case(LIBRARY_CFG) => return None,
        ABLETON_PREFS => {
            prefs_label(rel)?;
            "ableton/options"
        }
        _ => return None,
    })
}

/// This machine's choices.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncPrefs {
    /// The [`SyncKind::id`]s turned off; everything else syncs.
    #[serde(default)]
    pub off: BTreeSet<String>,
}

impl SyncPrefs {
    pub fn syncs(&self, id: &str) -> bool {
        !self.off.contains(id)
    }

    /// Whether the file `root`/`rel` syncs, by its kind.
    pub fn syncs_file(&self, root: &str, rel: &str) -> bool {
        kind_of(root, rel).is_none_or(|id| self.syncs(id))
    }

    pub fn set(&mut self, id: &str, on: bool) {
        if on {
            self.off.remove(id);
        } else if KINDS.iter().any(|k| k.id == id) {
            self.off.insert(id.to_owned());
        }
    }
}

pub fn load(app_dir: &Path) -> SyncPrefs {
    std::fs::read(app_dir.join(PREFS_FILE))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

pub fn save(app_dir: &Path, prefs: &SyncPrefs) -> Result<(), String> {
    std::fs::create_dir_all(app_dir).map_err(|e| e.to_string())?;
    let json = serde_json::to_vec_pretty(prefs).expect("prefs serialize");
    let tmp = app_dir.join(format!("{PREFS_FILE}.tmp"));
    std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, app_dir.join(PREFS_FILE)).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shared_file_has_a_listed_kind() {
        let ids: BTreeSet<&str> = KINDS.iter().map(|k| k.id).collect();
        assert_eq!(ids.len(), KINDS.len(), "ids are unique");
        for (root, rel) in [
            (REAPER_RESOURCES, "TrackTemplates/Drums.RTrackTemplate"),
            (REAPER_RESOURCES, "Scripts/ReaTeam Scripts/x.lua"),
            (REAPER_RESOURCES, "Effects/saike/x.jsfx"),
            (REAPER_RESOURCES, "Data/toolbar_icons/a.png"),
            (REAPER_RESOURCES, "reaper-kb.ini"),
            (REAPER_RESOURCES, "presets/vst3-ReaComp.ini"),
            (ABLETON_USER_LIBRARY, "Templates/Default.als"),
            (ABLETON_USER_LIBRARY, "Defaults/Audio Effects/EQ Eight.adv"),
            (
                ABLETON_USER_LIBRARY,
                "Presets/Instruments/Drum Rack/Kit.adg",
            ),
            (
                ABLETON_USER_LIBRARY,
                "Ableton Folder Info/12/Collections.xmp",
            ),
            (
                ABLETON_USER_LIBRARY,
                "Remote Scripts/My Controller/__init__.py",
            ),
            (ABLETON_PREFS, "Options.txt"),
            ("PRESETS_FABFILTER", "Pro-Q 4/Presets/Mine.ffp"),
        ] {
            let id = kind_of(root, rel).unwrap_or_else(|| panic!("{root}/{rel}"));
            assert!(ids.contains(id), "{id}");
        }
        assert_eq!(
            kind_of(REAPER_RESOURCES, "Data/toolbar_icons/a.png"),
            Some("reaper/toolbar-icons")
        );
        assert_eq!(kind_of(ABLETON_PREFS, LIBRARY_CFG), None);
        assert_eq!(kind_of(ABLETON_PREFS, "Preferences.cfg"), None);
    }

    #[test]
    fn choices_are_kept_per_machine() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(load(tmp.path()), SyncPrefs::default());
        let mut prefs = SyncPrefs::default();
        prefs.set("reaper/key-map", false);
        prefs.set("not a kind", false);
        save(tmp.path(), &prefs).unwrap();
        let back = load(tmp.path());
        assert!(!back.syncs("reaper/key-map"));
        assert!(back.syncs("not a kind"));
        assert!(!back.syncs_file(REAPER_RESOURCES, "reaper-kb.ini"));
        assert!(back.syncs_file(REAPER_RESOURCES, "reaper-menu.ini"));
        prefs.set("reaper/key-map", true);
        assert_eq!(prefs, SyncPrefs::default());
    }
}
