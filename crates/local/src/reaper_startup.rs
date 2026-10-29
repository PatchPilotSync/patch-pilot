//! Reaper's startup action on this machine (decision #107): what SWS runs
//! when Reaper opens here, read from `S&M.ini` and the key map now, against
//! the Primary Rig's. Setting it is a bring-over of `S&M.ini`, which writes
//! only the startup action into this machine's file, while Reaper is closed,
//! as a restore point.

use std::path::Path;

use patchpilot_core::{ContentHash, Manifest, Os, PortablePath};
use patchpilot_parity::reaper::{REAPER_RESOURCES, ResourceKind, portable_key_map};
use patchpilot_parity::reaper_startup::{self, ActionKind, STARTUP_SCRIPT, SWS_INI, StartupAction};
use serde::Serialize;

/// Where this machine stands. Mirrored in `app/src/api.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Nothing to do: the same as the Primary Rig's, or neither has one.
    Ok,
    /// The Primary Rig runs a startup action; this machine runs none.
    NotSet,
    /// This machine runs a different one.
    Different,
    /// This machine's startup action isn't one of its actions, so SWS can't
    /// run it: the custom action or script isn't in its key map.
    NotFound,
}

/// The startup action check. Mirrored in `app/src/api.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StartupCheck {
    pub status: Status,
    /// This machine's, as it is now.
    pub here: Option<StartupAction>,
    /// The Primary Rig's, as this machine's key map describes it, when this
    /// machine isn't the Primary Rig and it has one.
    pub primary: Option<StartupAction>,
    /// The Primary Rig has one, but it isn't one of this machine's actions
    /// yet: it comes with the Primary Rig's key map.
    pub primary_unknown: bool,
    /// The scripts the startup action here runs that aren't on this machine.
    pub missing_scripts: Vec<String>,
    /// The Primary Rig's `Scripts/__startup.lua` is missing here or differs.
    pub startup_script_differs: bool,
    /// There is a Primary Rig to bring files from (and this isn't it).
    pub has_primary: bool,
}

impl StartupCheck {
    /// Whether there is anything to show.
    pub fn needs_attention(&self) -> bool {
        self.status != Status::Ok || !self.missing_scripts.is_empty() || self.startup_script_differs
    }
}

/// The hash a manifest has for `rel` below Reaper's resource folder.
fn hash_of<'a>(m: &'a Manifest, rel: &str) -> Option<&'a ContentHash> {
    let path = PortablePath::logical(REAPER_RESOURCES, rel);
    m.items
        .iter()
        .find(|i| i.path == path)
        .and_then(|i| i.content_hash.as_ref())
}

/// Check the startup action in Reaper's resource folder `dir` (on `os`)
/// against `primary`, the Primary Rig through this machine's profile and
/// ignores, or `None` on the Primary Rig itself or before there is one.
pub fn check(dir: &Path, os: Os, primary: Option<&Manifest>) -> StartupCheck {
    let key_map = std::fs::read(dir.join(ResourceKind::KeyMap.folder()))
        .map(|kb| String::from_utf8_lossy(&portable_key_map(kb, dir, os)).into_owned())
        .unwrap_or_default();
    let sws = std::fs::read(dir.join(SWS_INI)).unwrap_or_default();
    let here = std::str::from_utf8(&sws)
        .ok()
        .and_then(reaper_startup::global_action)
        .map(|id| reaper_startup::describe(id, &key_map));
    let missing_scripts: Vec<String> = here
        .iter()
        .flat_map(|a| &a.scripts)
        .filter(|rel| {
            !rel.split('/')
                .fold(dir.to_owned(), |p, s| p.join(s))
                .is_file()
        })
        .cloned()
        .collect();

    // The Primary Rig's is known by its hash: it is whichever action this
    // machine has (or the one here) whose shared part hashes the same.
    let want = primary.and_then(|p| hash_of(p, SWS_INI));
    let primary_action = want.and_then(|want| {
        reaper_startup::defined_ids(&key_map)
            .into_iter()
            .chain(here.as_ref().map(|a| a.id.clone()))
            .find(|id| &patchpilot_sync::content_hash(&reaper_startup::view_for(id)) == want)
            .map(|id| reaper_startup::describe(&id, &key_map))
    });
    let same = match (want, &here) {
        (Some(want), Some(h)) => {
            &patchpilot_sync::content_hash(&reaper_startup::view_for(&h.id)) == want
        }
        _ => false,
    };
    let status = match (&want, &here) {
        (Some(_), None) => Status::NotSet,
        (Some(_), Some(_)) if !same => Status::Different,
        (_, Some(h)) if h.kind == ActionKind::Missing => Status::NotFound,
        _ => Status::Ok,
    };

    let startup_script_differs = primary
        .and_then(|p| hash_of(p, STARTUP_SCRIPT))
        .is_some_and(|want| {
            crate::bring::shared_hash(dir, REAPER_RESOURCES, STARTUP_SCRIPT).as_ref() != Some(want)
        });

    StartupCheck {
        status,
        primary_unknown: want.is_some() && primary_action.is_none(),
        primary: primary_action,
        here,
        missing_scripts,
        startup_script_differs,
        has_primary: primary.is_some(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patchpilot_core::{Confidence, ItemKind, MachineInfo, PathMap, ProductId};

    const KB: &str = "ACT 0 0 \"aa\" \"Custom: JMD_Startup\" _RS1\n\
                      ACT 0 0 \"bb\" \"Custom: Other\" 40044\n\
                      SCR 4 0 RS1 \"Custom: folder.lua\" \"nvk/folder.lua\"\n";

    fn primary(files: &[(&str, &[u8])]) -> Manifest {
        let mut m = Manifest::new(
            MachineInfo {
                id: "pc".into(),
                name: "PC".into(),
                os: Os::Windows,
                os_version: None,
                arch: patchpilot_core::Arch::X86_64,
                path_map: PathMap::new(Os::Windows),
                disks: Vec::new(),
            },
            0,
        );
        m.items = files
            .iter()
            .map(|(rel, shared)| patchpilot_core::Item {
                kind: ItemKind::DawAsset,
                product_id: Some(ProductId(format!("reaper-config/{rel}"))),
                format: None,
                platform: Os::Windows,
                arch: None,
                version: None,
                path: PortablePath::logical(REAPER_RESOURCES, *rel),
                content_hash: Some(patchpilot_sync::content_hash(shared)),
                size: Some(shared.len() as u64),
                mtime: None,
                platform_portable: true,
                confidence: Confidence::High,
                requires: Vec::new(),
                needs_files: Vec::new(),
            })
            .collect();
        m
    }

    fn here(sws: Option<&str>, scripts: bool) -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("reaper-kb.ini"), KB).unwrap();
        if let Some(sws) = sws {
            std::fs::write(tmp.path().join("S&M.ini"), sws).unwrap();
        }
        if scripts {
            std::fs::create_dir_all(tmp.path().join("Scripts/nvk")).unwrap();
            std::fs::write(tmp.path().join("Scripts/nvk/folder.lua"), "--").unwrap();
        }
        tmp
    }

    #[test]
    fn a_machine_without_the_primary_rigs_startup_action_is_told() {
        let pc = primary(&[("S&M.ini", &reaper_startup::view_for("_aa"))]);
        let mac = here(Some("[General]\nBeta=0\n"), true);
        let c = check(mac.path(), Os::Macos, Some(&pc));
        assert_eq!(c.status, Status::NotSet);
        assert!(c.here.is_none());
        let p = c.primary.unwrap();
        assert_eq!(p.name.as_deref(), Some("Custom: JMD_Startup"));
        assert_eq!(p.scripts, ["Scripts/nvk/folder.lua"]);
        assert!(!c.primary_unknown);
    }

    #[test]
    fn the_same_action_is_fine_and_a_different_one_is_named() {
        let pc = primary(&[("S&M.ini", &reaper_startup::view_for("_aa"))]);
        let mac = here(Some("[Misc]\nGlobalStartupAction=_aa\n"), true);
        let c = check(mac.path(), Os::Macos, Some(&pc));
        assert_eq!(c.status, Status::Ok);
        assert!(!c.needs_attention());

        let mac = here(Some("[Misc]\nGlobalStartupAction=_bb\n"), true);
        let c = check(mac.path(), Os::Macos, Some(&pc));
        assert_eq!(c.status, Status::Different);
        assert_eq!(c.here.unwrap().name.as_deref(), Some("Custom: Other"));
        assert_eq!(
            c.primary.unwrap().name.as_deref(),
            Some("Custom: JMD_Startup")
        );
    }

    #[test]
    fn an_action_this_machine_lacks_comes_with_the_key_map() {
        let pc = primary(&[(
            "S&M.ini",
            &reaper_startup::view_for("_0123456789abcdef0123456789abcdef"),
        )]);
        let mac = here(None, false);
        let c = check(mac.path(), Os::Macos, Some(&pc));
        assert_eq!(c.status, Status::NotSet);
        assert!(c.primary.is_none());
        assert!(c.primary_unknown);
    }

    #[test]
    fn startup_actions_that_cant_run_here_are_named() {
        // Not in this key map.
        let mac = here(
            Some("[Misc]\nGlobalStartupAction=_0123456789abcdef0123456789abcdef\n"),
            true,
        );
        let c = check(mac.path(), Os::Macos, None);
        assert_eq!(c.status, Status::NotFound);
        // In it, but its script isn't here.
        let mac = here(Some("[Misc]\nGlobalStartupAction=_aa\n"), false);
        let c = check(mac.path(), Os::Macos, None);
        assert_eq!(c.status, Status::Ok);
        assert_eq!(c.missing_scripts, ["Scripts/nvk/folder.lua"]);
        assert!(c.needs_attention());
    }

    #[test]
    fn the_primary_rigs_startup_script_is_checked_too() {
        let pc = primary(&[(STARTUP_SCRIPT, b"reaper.Main_OnCommand(40044, 0)")]);
        let mac = here(None, false);
        assert!(check(mac.path(), Os::Macos, Some(&pc)).startup_script_differs);
        std::fs::create_dir_all(mac.path().join("Scripts")).unwrap();
        std::fs::write(
            mac.path().join(STARTUP_SCRIPT),
            "reaper.Main_OnCommand(40044, 0)",
        )
        .unwrap();
        let c = check(mac.path(), Os::Macos, Some(&pc));
        assert!(!c.startup_script_differs);
        assert!(!c.needs_attention());
    }
}
