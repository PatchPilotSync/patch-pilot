//! ReaPack and Reaper folder settings on this machine: reading them into the
//! manifest at scan time, and the file changes that bring them in line with
//! the Golden Rig (decision #43).
//!
//! Reading is read-only (ReaPack's registry is opened read-only). Nothing
//! here writes: [`reapack_changes`] and [`folder_changes`] return the new
//! file contents, and the caller writes them through the restore-point
//! writer, which refuses while Reaper is running and backs up first.

use std::path::{Path, PathBuf};

use patchpilot_core::{Manifest, Os};
use patchpilot_parity::reapack::{self, Package, ReaPackRestore};
use patchpilot_parity::reaper_folders::{self, FolderFix, LocalFolders};
use rusqlite::{Connection, OpenFlags};

/// Logical root for the user's home folder, so `~/Documents/REAPER Media`
/// means the same on every machine.
pub const HOME: &str = "HOME";

const REAPACK_INI: &str = "reapack.ini";
const REAPER_INI: &str = "reaper.ini";

fn registry_path(resource_dir: &Path) -> PathBuf {
    resource_dir.join("ReaPack").join("registry.db")
}

/// Add ReaPack repositories and packages and Reaper's folder settings under
/// `dir` to `manifest`. Returns warnings for files that couldn't be read.
pub fn add_reaper_extras(manifest: &mut Manifest, dir: &Path, home: Option<&str>) -> Vec<String> {
    let mut warnings = Vec::new();
    let os = manifest.machine.os;
    let map = &mut manifest.machine.path_map;
    map.roots.insert(
        patchpilot_parity::reaper::REAPER_RESOURCES.to_owned(),
        dir.to_string_lossy().into_owned(),
    );
    if let Some(home) = home {
        map.roots.insert(HOME.to_owned(), home.to_owned());
    }

    if let Ok(text) = std::fs::read_to_string(dir.join(REAPER_INI)) {
        let settings = reaper_folders::read_folder_settings(&text);
        let (products, items) = reaper_folders::folder_items(&settings, &manifest.machine.path_map);
        manifest.products.extend(products);
        manifest.items.extend(items);
    }

    let remotes = match std::fs::read_to_string(dir.join(REAPACK_INI)) {
        Ok(text) => reapack::parse_remotes(&text),
        Err(_) => return warnings,
    };
    let packages = match read_registry(&registry_path(dir)) {
        Ok(p) => p,
        Err(e) => {
            warnings.push(format!("ReaPack's package list: {e}"));
            Vec::new()
        }
    };
    let (products, items) = reapack::reapack_items(&remotes, &packages, os);
    manifest.products.extend(products);
    manifest.items.extend(items);
    warnings
}

/// Installed packages in ReaPack's `registry.db`. A missing file means none.
pub fn read_registry(db: &Path) -> Result<Vec<Package>, String> {
    if !db.is_file() {
        return Ok(Vec::new());
    }
    let conn = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| e.to_string())?;
    let cols = entry_columns(&conn)?;
    let has = |c: &str| cols.iter().any(|x| x == c);
    let opt = |c: &str, default: &str| {
        if has(c) {
            c.to_owned()
        } else {
            default.to_owned()
        }
    };
    let sql = format!(
        "SELECT remote, category, package, {}, {}, version FROM entries ORDER BY remote, category, package",
        opt("desc", "''"),
        opt("type", "0"),
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            Ok(Package {
                remote: r.get(0)?,
                category: r.get(1)?,
                package: r.get(2)?,
                desc: r.get(3)?,
                kind: r.get(4)?,
                version: r.get(5)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
}

fn entry_columns(conn: &Connection) -> Result<Vec<String>, String> {
    let mut stmt = conn
        .prepare("SELECT name FROM pragma_table_info('entries')")
        .map_err(|e| e.to_string())?;
    let cols: Vec<String> = stmt
        .query_map([], |r| r.get(0))
        .and_then(|r| r.collect())
        .map_err(|e| e.to_string())?;
    let needed = ["remote", "category", "package", "version"];
    if needed.iter().all(|n| cols.iter().any(|c| c == n)) {
        Ok(cols)
    } else {
        Err("ReaPack's registry is in a format Patch Pilot doesn't know".to_owned())
    }
}

/// ReaPack and folder fixes for this machine against Golden.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ReaperExtras {
    /// Reaper's resource folder exists here.
    pub reaper_found: bool,
    /// ReaPack has run here (`reapack.ini` exists).
    pub reapack_installed: bool,
    pub reapack: ReaPackRestore,
    pub folders: Vec<FolderFix>,
}

impl ReaperExtras {
    pub fn is_empty(&self) -> bool {
        self.reapack.is_empty() && self.folders.is_empty()
    }
}

/// What this machine (`machine`, scanned from `dir`) needs to match `golden`.
pub fn reaper_extras(golden: &Manifest, machine: &Manifest, dir: Option<&Path>) -> ReaperExtras {
    let bases = local_bases(machine.machine.os);
    let exists = |p: &str| Path::new(p).is_dir();
    let local = LocalFolders {
        bases,
        exists: &exists,
    };
    ReaperExtras {
        reaper_found: dir.is_some_and(Path::is_dir),
        reapack_installed: dir.is_some_and(|d| d.join(REAPACK_INI).is_file()),
        reapack: reapack::restore_plan(golden, machine),
        folders: reaper_folders::folder_plan(golden, machine, &local),
    }
}

/// Drive roots or volumes, then the home folder.
fn local_bases(os: Os) -> Vec<String> {
    let mut out = Vec::new();
    match os {
        Os::Windows => {
            for letter in b'C'..=b'Z' {
                let root = format!("{}:\\", letter as char);
                if Path::new(&root).is_dir() {
                    out.push(root);
                }
            }
        }
        Os::Macos => {
            if let Ok(entries) = std::fs::read_dir("/Volumes") {
                let mut vols: Vec<String> = entries
                    .flatten()
                    .filter(|e| e.path().is_dir())
                    .map(|e| e.path().to_string_lossy().into_owned())
                    .collect();
                vols.sort();
                out.extend(vols);
            }
            out.push("/".to_owned());
        }
    }
    out.extend(crate::home_dir());
    out
}

/// A file Patch Pilot would write, with its new contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewFile {
    pub path: PathBuf,
    pub contents: Vec<u8>,
}

/// New `reapack.ini` (missing repositories added) and `registry.db` (missing
/// packages listed at version 0.0, so ReaPack's "Synchronize packages"
/// installs them as updates). Needs ReaPack to have run once here.
pub fn reapack_changes(dir: &Path, restore: &ReaPackRestore) -> Result<Vec<NewFile>, String> {
    let ini_path = dir.join(REAPACK_INI);
    let ini = std::fs::read_to_string(&ini_path)
        .map_err(|_| "Install ReaPack in Reaper first (reapack.com), then try again".to_owned())?;
    let mut out = Vec::new();
    if !restore.missing_remotes.is_empty() {
        out.push(NewFile {
            path: ini_path,
            contents: reapack::add_remotes(&ini, &restore.missing_remotes).into_bytes(),
        });
    }
    if !restore.missing_packages.is_empty() {
        let db = registry_path(dir);
        if !db.is_file() {
            return Err(
                "Open Reaper once with ReaPack installed so it creates its package list, then try again"
                    .to_owned(),
            );
        }
        out.push(NewFile {
            contents: seeded_registry(&db, &restore.missing_packages)?,
            path: db,
        });
    }
    Ok(out)
}

/// The registry's bytes with `packages` added, built on a temporary copy.
fn seeded_registry(db: &Path, packages: &[Package]) -> Result<Vec<u8>, String> {
    let tmp = tempfile::tempdir().map_err(|e| e.to_string())?;
    let copy = tmp.path().join("registry.db");
    std::fs::copy(db, &copy).map_err(|e| e.to_string())?;
    {
        let conn = Connection::open(&copy).map_err(|e| e.to_string())?;
        let cols = entry_columns(&conn)?;
        let has = |c: &str| cols.iter().any(|x| x == c);
        let mut names = vec!["remote", "category", "package", "version"];
        names.extend(
            ["desc", "type", "author", "flags"]
                .into_iter()
                .filter(|c| has(c)),
        );
        let sql = format!(
            "INSERT OR IGNORE INTO entries ({}) VALUES ({})",
            names.join(", "),
            (1..=names.len())
                .map(|n| format!("?{n}"))
                .collect::<Vec<_>>()
                .join(", ")
        );
        for p in packages {
            let mut values: Vec<rusqlite::types::Value> = vec![
                p.remote.clone().into(),
                p.category.clone().into(),
                p.package.clone().into(),
                // Older than any real version, so Synchronize installs it.
                "0.0".to_owned().into(),
            ];
            for c in &names[4..] {
                values.push(match *c {
                    "desc" => p.desc.clone().into(),
                    "type" => p.kind.into(),
                    "author" => String::new().into(),
                    _ => 0i64.into(),
                });
            }
            conn.execute(&sql, rusqlite::params_from_iter(values))
                .map_err(|e| e.to_string())?;
        }
    }
    std::fs::read(&copy).map_err(|e| e.to_string())
}

/// New `reaper.ini` with each fix that found a folder applied, or `None`
/// when there is nothing to change.
pub fn folder_changes(dir: &Path, fixes: &[FolderFix]) -> Result<Option<NewFile>, String> {
    if fixes.iter().all(|f| f.proposed.is_none()) {
        return Ok(None);
    }
    let path = dir.join(REAPER_INI);
    let text = std::fs::read_to_string(&path).map_err(|e| format!("reaper.ini: {e}"))?;
    let new = reaper_folders::apply_folder_fixes(&text, fixes);
    Ok((new != text).then(|| NewFile {
        path,
        contents: new.into_bytes(),
    }))
}

#[cfg(test)]
mod tests {
    use patchpilot_core::{Arch, MachineInfo, PathMap};

    use patchpilot_parity::reapack::Remote;

    use super::*;

    const SCHEMA: &str = "CREATE TABLE entries (
        id INTEGER PRIMARY KEY, remote TEXT NOT NULL, category TEXT NOT NULL,
        package TEXT NOT NULL, desc TEXT NOT NULL, type INTEGER NOT NULL,
        version TEXT NOT NULL, author TEXT NOT NULL, flags INTEGER DEFAULT 0,
        UNIQUE(remote, category, package));
      CREATE TABLE files (id INTEGER PRIMARY KEY, entry INTEGER NOT NULL,
        path TEXT UNIQUE NOT NULL, main INTEGER NOT NULL, type INTEGER NOT NULL);";

    fn resource_dir(packages: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(REAPACK_INI),
            "[remotes]\nremote0=ReaTeam Scripts|https://github.com/ReaTeam/ReaScripts/raw/master/index.xml|1|2\nsize=1\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join(REAPER_INI),
            "[REAPER]\ndefrecpath=C:\\Users\\josh\\Rec\n",
        )
        .unwrap();
        std::fs::create_dir(dir.path().join("ReaPack")).unwrap();
        let conn = Connection::open(registry_path(dir.path())).unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        for (pkg, version) in packages {
            conn.execute(
                "INSERT INTO entries (remote, category, package, desc, type, version, author)
                 VALUES ('ReaTeam Scripts', 'Items Editing', ?1, ?1, 1, ?2, 'someone')",
                [pkg, version],
            )
            .unwrap();
        }
        dir
    }

    fn scan(dir: &Path) -> Manifest {
        let mut m = Manifest::new(
            MachineInfo {
                id: "m".into(),
                name: "M".into(),
                os: Os::Windows,
                arch: Arch::X86_64,
                os_version: None,
                path_map: PathMap::new(Os::Windows),
            },
            0,
        );
        let warnings = add_reaper_extras(&mut m, dir, Some(r"C:\Users\josh"));
        assert!(warnings.is_empty(), "{warnings:?}");
        m
    }

    #[test]
    fn scans_reapack_and_folders() {
        let dir = resource_dir(&[("Script: Foo.lua", "1.2"), ("Script: Bar.lua", "2.0")]);
        let m = scan(dir.path());
        let ids: Vec<_> = m
            .items
            .iter()
            .filter_map(|i| i.product_id.as_ref())
            .map(|p| p.0.as_str())
            .collect();
        assert_eq!(
            ids,
            [
                "reaper-folder/defrecpath",
                "reapack-repo/reateam-scripts",
                "reapack/reateam-scripts/items-editing/script-bar-lua",
                "reapack/reateam-scripts/items-editing/script-foo-lua",
            ]
        );
        assert_eq!(m.machine.path_map.roots[HOME], r"C:\Users\josh");
    }

    #[test]
    fn seeds_missing_packages_for_reapack_to_install() {
        let golden =
            scan(resource_dir(&[("Script: Foo.lua", "1.2"), ("Script: Bar.lua", "2.0")]).path());
        let here = resource_dir(&[("Script: Foo.lua", "1.2")]);
        let machine = scan(here.path());
        let restore = reapack::restore_plan(&golden, &machine);
        assert_eq!(restore.missing_packages.len(), 1);

        let files = reapack_changes(here.path(), &restore).unwrap();
        assert_eq!(files.len(), 1, "the repository is already there");
        std::fs::write(&files[0].path, &files[0].contents).unwrap();

        let after = read_registry(&registry_path(here.path())).unwrap();
        let bar = after
            .iter()
            .find(|p| p.package == "Script: Bar.lua")
            .unwrap();
        assert_eq!(bar.version, "0.0");
        assert_eq!(after.len(), 2);
        let rescanned = scan(here.path());
        assert!(reapack::restore_plan(&golden, &rescanned).is_empty());
    }

    #[test]
    fn needs_reapack_installed_to_restore() {
        let dir = tempfile::tempdir().unwrap();
        let restore = ReaPackRestore {
            missing_remotes: vec![Remote {
                name: "A".into(),
                url: "https://a/index.xml".into(),
                enabled: true,
            }],
            missing_packages: Vec::new(),
        };
        assert!(reapack_changes(dir.path(), &restore).is_err());
    }
}
