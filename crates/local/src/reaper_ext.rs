//! Reaper extensions on this machine (decision #104): the files in
//! `UserPlugins/` as manifest items at scan time, and the live check of
//! whether each one loads, with its one-click fixes.
//!
//! Reading takes each file's name, its first bytes (the Mach-O or PE
//! header) and, on a Mac, its quarantine flag; extensions are never loaded.
//! The fixes that change files ([`fix_changes`]) return file changes for the
//! restore-point writer, which refuses while Reaper is open and backs up
//! first. Unblocking only clears macOS's quarantine flag on the one file.

use std::io::Read;
use std::path::{Path, PathBuf};

use patchpilot_core::{Arch, Manifest, Os};
use patchpilot_parity::reaper_ext::{
    self, Cpu, ExtensionState, FixAction, FoundFile, HEAD_BYTES, USER_PLUGINS,
};

use crate::restore::FileChange;

/// How deep below `UserPlugins/` to look for files dropped in a subfolder.
const MAX_DEPTH: usize = 3;

/// The extended attribute macOS puts on downloaded files.
#[cfg(target_os = "macos")]
const QUARANTINE: &str = "com.apple.quarantine";

/// One extension file, with where it is.
#[derive(Debug, Clone)]
struct Found {
    path: PathBuf,
    file: FoundFile,
}

/// Every `reaper_*` extension file in `UserPlugins/` under Reaper's
/// resource folder `dir`, and in its subfolders a few levels down.
fn find(dir: &Path) -> Vec<Found> {
    let root = dir.join(USER_PLUGINS);
    let mut out = Vec::new();
    let walk = walkdir::WalkDir::new(&root)
        .max_depth(MAX_DEPTH + 1)
        .follow_links(true)
        .into_iter()
        .filter_entry(|e| e.depth() == 0 || !e.file_name().to_string_lossy().starts_with('.'));
    for entry in walk.filter_map(Result::ok) {
        if !entry.file_type().is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy();
        if reaper_ext::parse_file_name(&name).is_none() {
            continue;
        }
        let Ok(rel) = entry.path().strip_prefix(&root) else {
            continue;
        };
        let rel: Vec<String> = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        let path = entry.into_path();
        out.push(Found {
            file: FoundFile {
                rel: rel.join("/"),
                binary: head(&path).as_deref().and_then(reaper_ext::read_binary),
                quarantined: quarantined(&path),
                mtime: mtime(&path).unwrap_or(0),
            },
            path,
        });
    }
    out.sort_by(|a, b| a.file.rel.cmp(&b.file.rel));
    out
}

fn head(path: &Path) -> Option<Vec<u8>> {
    let mut buf = Vec::with_capacity(HEAD_BYTES);
    std::fs::File::open(path)
        .ok()?
        .take(HEAD_BYTES as u64)
        .read_to_end(&mut buf)
        .ok()?;
    Some(buf)
}

fn mtime(path: &Path) -> Option<i64> {
    let t = std::fs::metadata(path).ok()?.modified().ok()?;
    Some(t.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs() as i64)
}

/// Whether a `com.apple.quarantine` value still blocks the file: its first
/// field is hex flags, and 0x40 means the user allowed it.
pub fn quarantine_blocks(value: &[u8]) -> bool {
    let text = String::from_utf8_lossy(value);
    let flags = text.split(';').next().unwrap_or("");
    u32::from_str_radix(flags.trim(), 16).map_or(true, |f| f & 0x40 == 0)
}

#[cfg(target_os = "macos")]
fn quarantined(path: &Path) -> bool {
    matches!(xattr::get(path, QUARANTINE), Ok(Some(v)) if quarantine_blocks(&v))
}

#[cfg(not(target_os = "macos"))]
fn quarantined(_path: &Path) -> bool {
    false
}

/// The CPU Reaper runs as here, from which plug-in list it wrote last.
pub fn reaper_cpu(dir: &Path, os: Os, arch: Arch) -> Cpu {
    reaper_ext::reaper_cpu(
        os,
        arch,
        mtime(&dir.join("reaper-vstplugins_arm64.ini")),
        mtime(&dir.join("reaper-vstplugins64.ini")),
    )
}

/// Add the extensions Reaper loads from `dir`'s `UserPlugins/` to
/// `manifest`: the files at its top built for this OS and the CPU Reaper
/// runs as. Copies in subfolders and other builds don't count as installed;
/// the live check names them.
pub fn add_extensions(manifest: &mut Manifest, dir: &Path) {
    let os = manifest.machine.os;
    let cpu = reaper_cpu(dir, os, manifest.machine.arch);
    for f in find(dir) {
        if !f.file.loads(os, cpu) {
            continue;
        }
        let (Some(name), Some(binary)) = (
            reaper_ext::parse_file_name(&f.file.rel),
            f.file.binary.as_ref(),
        ) else {
            continue;
        };
        let version = match os {
            Os::Windows => patchpilot_scanner::read_pe_version(&f.path)
                .ok()
                .and_then(|v| v.version().map(str::to_owned)),
            Os::Macos => None,
        };
        let size = std::fs::metadata(&f.path).ok().map(|m| m.len());
        let (product, item) = reaper_ext::extension_item(
            &name,
            &f.file.rel,
            binary,
            version,
            size,
            Some(f.file.mtime),
        );
        if !manifest
            .products
            .iter()
            .any(|p| p.canonical_id == product.canonical_id)
        {
            manifest.products.push(product);
        }
        manifest.items.push(item);
    }
}

/// The extensions check for this machine.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ExtensionCheck {
    /// Reaper's `UserPlugins/` folder here, as the OS spells it.
    pub folder: String,
    pub reaper_cpu: Cpu,
    /// Problems first, then the extensions that load.
    pub extensions: Vec<ExtensionState>,
}

impl ExtensionCheck {
    /// How many extensions need something done.
    pub fn problems(&self) -> usize {
        self.extensions
            .iter()
            .filter(|e| e.status != reaper_ext::Status::Ok)
            .count()
    }
}

/// Check every extension in Reaper's resource folder `dir`, and the ones
/// the scripts in `machine` (this machine's latest scan) call.
pub fn check(dir: &Path, os: Os, arch: Arch, machine: Option<&Manifest>) -> ExtensionCheck {
    let cpu = reaper_cpu(dir, os, arch);
    let files: Vec<FoundFile> = find(dir).into_iter().map(|f| f.file).collect();
    let needed = machine
        .map(reaper_ext::needed_by_scripts)
        .unwrap_or_default();
    ExtensionCheck {
        folder: dir.join(USER_PLUGINS).to_string_lossy().into_owned(),
        reaper_cpu: cpu,
        extensions: reaper_ext::check(&files, os, cpu, &needed),
    }
}

/// What a fix will do.
pub enum FixPlan {
    /// Clear the quarantine flag on this file.
    Unblock(PathBuf),
    /// Write these through a restore point, with this History label.
    Files(String, Vec<FileChange>),
}

/// The fix the check offers for `file` (below `UserPlugins/`) with
/// `action`, worked out again from the files as they are now, so only a fix
/// the check itself offers runs.
pub fn fix_plan(
    dir: &Path,
    os: Os,
    arch: Arch,
    file: &str,
    action: FixAction,
) -> Result<FixPlan, String> {
    let cpu = reaper_cpu(dir, os, arch);
    let found = find(dir);
    let files: Vec<FoundFile> = found.iter().map(|f| f.file.clone()).collect();
    let states = reaper_ext::check(&files, os, cpu, &Default::default());
    let state = states
        .iter()
        .find(|s| {
            s.fix
                .as_ref()
                .is_some_and(|f| f.action == action && f.file == file)
        })
        .ok_or("That's already sorted. Check again to see what's left.")?;
    let path = found
        .iter()
        .find(|f| f.file.rel == file)
        .map(|f| f.path.clone())
        .ok_or("That file is gone")?;
    Ok(match action {
        FixAction::Unblock => FixPlan::Unblock(path),
        FixAction::MoveUp => {
            let name = file.rsplit('/').next().unwrap_or(file);
            let bytes = std::fs::read(&path).map_err(|e| format!("{file}: {e}"))?;
            FixPlan::Files(
                format!("Moved {} into UserPlugins", state.name),
                vec![
                    FileChange {
                        path: dir.join(USER_PLUGINS).join(name),
                        contents: Some(bytes),
                    },
                    FileChange {
                        path,
                        contents: None,
                    },
                ],
            )
        }
        FixAction::Remove => FixPlan::Files(
            format!("Removed the older copy of {}", state.name),
            vec![FileChange {
                path,
                contents: None,
            }],
        ),
    })
}

/// Clear macOS's quarantine flag on one file, so Reaper may load it.
#[cfg(target_os = "macos")]
pub fn unblock(path: &Path) -> Result<(), String> {
    xattr::remove(path, QUARANTINE).or_else(|e| match xattr::get(path, QUARANTINE) {
        // Already gone.
        Ok(None) => Ok(()),
        _ => Err(format!("Couldn't unblock it: {e}")),
    })
}

#[cfg(not(target_os = "macos"))]
pub fn unblock(_path: &Path) -> Result<(), String> {
    Err("Only macOS blocks downloaded extensions".to_owned())
}

#[cfg(test)]
mod tests {
    use patchpilot_core::{MachineInfo, PathMap};
    use patchpilot_parity::reaper_ext::Status;

    use super::*;

    fn macho_arm64() -> Vec<u8> {
        let mut b = vec![0xCF, 0xFA, 0xED, 0xFE, 0x0C, 0x00, 0x00, 0x01];
        b.extend([0; 64]);
        b
    }

    fn pe_x64() -> Vec<u8> {
        let mut b = vec![0u8; 0x80];
        b[..2].copy_from_slice(b"MZ");
        b[0x3C] = 0x40;
        b[0x40..0x44].copy_from_slice(b"PE\0\0");
        b[0x44..0x46].copy_from_slice(&0x8664u16.to_le_bytes());
        b
    }

    fn resource_dir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("UserPlugins/sws-2.14")).unwrap();
        dir
    }

    fn write(dir: &Path, rel: &str, bytes: &[u8]) {
        let path = dir.join(USER_PLUGINS).join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    fn manifest(os: Os, arch: Arch) -> Manifest {
        Manifest::new(
            MachineInfo {
                id: "m".into(),
                name: "M".into(),
                os,
                arch,
                os_version: None,
                path_map: PathMap::new(os),
                disks: Vec::new(),
            },
            0,
        )
    }

    #[test]
    fn quarantine_blocks_until_allowed() {
        assert!(quarantine_blocks(b"0083;65f1a2b3;Safari;1234-ABCD"));
        assert!(quarantine_blocks(b"0081;65f1a2b3;Chrome;"));
        assert!(!quarantine_blocks(b"00c1;65f1a2b3;Safari;1234"));
        assert!(quarantine_blocks(b"junk"));
    }

    #[test]
    fn only_loadable_files_count_as_installed() {
        let dir = resource_dir();
        write(dir.path(), "reaper_reapack-arm64.dylib", &macho_arm64());
        write(dir.path(), "reaper_sws-x64.dll", &pe_x64());
        write(
            dir.path(),
            "sws-2.14/reaper_sws-arm64.dylib",
            &macho_arm64(),
        );
        write(dir.path(), "sws_python64.py", b"print()");
        let mut m = manifest(Os::Macos, Arch::Aarch64);
        add_extensions(&mut m, dir.path());
        let ids: Vec<_> = m
            .items
            .iter()
            .map(|i| i.product_id.as_ref().unwrap().0.as_str())
            .collect();
        assert_eq!(ids, ["reaper-ext/reapack"]);
        assert_eq!(m.products[0].name, "ReaPack");
    }

    #[test]
    fn the_check_names_each_problem_and_its_fix() {
        let dir = resource_dir();
        write(dir.path(), "reaper_reapack-arm64.dylib", &macho_arm64());
        write(
            dir.path(),
            "sws-2.14/reaper_sws-arm64.dylib",
            &macho_arm64(),
        );
        write(dir.path(), "reaper_imgui-arm64.dylib", b"");
        let c = check(dir.path(), Os::Macos, Arch::Aarch64, None);
        let got: Vec<_> = c
            .extensions
            .iter()
            .map(|e| (e.name.as_str(), e.status))
            .collect();
        assert_eq!(
            got,
            [
                ("SWS Extension", Status::InSubfolder),
                ("ReaImGui", Status::Damaged),
                ("ReaPack", Status::Ok),
            ]
        );
        assert_eq!(c.problems(), 2);

        let plan = fix_plan(
            dir.path(),
            Os::Macos,
            Arch::Aarch64,
            "sws-2.14/reaper_sws-arm64.dylib",
            FixAction::MoveUp,
        )
        .unwrap();
        let FixPlan::Files(label, changes) = plan else {
            panic!("a move writes files");
        };
        assert_eq!(label, "Moved SWS Extension into UserPlugins");
        assert_eq!(
            changes[0].path,
            dir.path()
                .join("UserPlugins")
                .join("reaper_sws-arm64.dylib")
        );
        assert_eq!(changes[0].contents.as_deref(), Some(&macho_arm64()[..]));
        assert!(changes[1].contents.is_none());
        for c in &changes {
            match &c.contents {
                Some(b) => std::fs::write(&c.path, b).unwrap(),
                None => std::fs::remove_file(&c.path).unwrap(),
            }
        }
        let c = check(dir.path(), Os::Macos, Arch::Aarch64, None);
        let sws = c.extensions.iter().find(|e| e.name == "SWS Extension");
        assert_eq!(sws.unwrap().status, Status::Ok);
        assert_eq!(c.problems(), 1);

        // A fix the check doesn't offer is refused.
        assert!(
            fix_plan(
                dir.path(),
                Os::Macos,
                Arch::Aarch64,
                "reaper_reapack-arm64.dylib",
                FixAction::Remove,
            )
            .is_err()
        );
    }

    #[test]
    fn scripts_here_ask_for_what_they_call() {
        let dir = resource_dir();
        let mut m = manifest(Os::Windows, Arch::X86_64);
        let mut item = patchpilot_core::Item::plugin(
            patchpilot_core::PluginFormat::Vst3,
            Os::Windows,
            patchpilot_core::PortablePath::logical(
                patchpilot_parity::reaper::REAPER_RESOURCES,
                "Scripts/nvk/FOLDER_ITEMS.lua",
            ),
        );
        item.kind = patchpilot_core::ItemKind::DawAsset;
        item.format = None;
        item.requires = vec![reaper_ext::product_id("sws")];
        m.items.push(item);
        let c = check(dir.path(), Os::Windows, Arch::X86_64, Some(&m));
        assert_eq!(c.extensions.len(), 1);
        assert_eq!(c.extensions[0].status, Status::Missing);
        assert_eq!(c.extensions[0].needed_by, ["Scripts/nvk/FOLDER_ITEMS.lua"]);
        assert_eq!(
            c.extensions[0].download.as_ref().unwrap().file.as_deref(),
            Some("reaper_sws-x64.dll")
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn unblocks_a_downloaded_file() {
        let dir = resource_dir();
        write(dir.path(), "reaper_sws-arm64.dylib", &macho_arm64());
        let path = dir.path().join("UserPlugins/reaper_sws-arm64.dylib");
        xattr::set(&path, QUARANTINE, b"0083;65f1a2b3;Safari;1234").unwrap();
        let c = check(dir.path(), Os::Macos, Arch::Aarch64, None);
        assert_eq!(c.extensions[0].status, Status::Blocked);
        let FixPlan::Unblock(p) = fix_plan(
            dir.path(),
            Os::Macos,
            Arch::Aarch64,
            "reaper_sws-arm64.dylib",
            FixAction::Unblock,
        )
        .unwrap() else {
            panic!("unblock");
        };
        unblock(&p).unwrap();
        let c = check(dir.path(), Os::Macos, Arch::Aarch64, None);
        assert_eq!(c.extensions[0].status, Status::Ok);
    }
}
