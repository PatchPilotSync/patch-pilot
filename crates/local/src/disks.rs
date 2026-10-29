//! How full this machine's drives are (decision #101): every local drive
//! (decision #102), each with what Patch Pilot found on it (plug-ins,
//! presets, DAW files, home folder, Installer Vault), so every linked machine
//! can show it. Asks the OS for each drive's numbers; never walks a drive.

use std::path::{Path, PathBuf};

use patchpilot_core::{Disk, MachineInfo};

/// A reading that differs from the stored one by this much is worth keeping
/// and sending to linked machines.
const MOVED_BYTES: u64 = 1 << 30;
/// A reading older than this is refreshed even when nothing moved, so other
/// machines don't show a days-old number.
const STALE_SECS: i64 = 24 * 3600;
/// Room left over after a write, so a bring-over never fills a drive.
const HEADROOM: u64 = 256 << 20;

/// The folders whose drives matter on a machine, each with what it holds:
/// every logical root of its path map, plus its home folder and Installer
/// Vault when known.
pub fn places(
    machine: &MachineInfo,
    home: Option<&str>,
    vault: Option<&Path>,
) -> Vec<(PathBuf, &'static str)> {
    let mut v: Vec<(PathBuf, &'static str)> = machine
        .path_map
        .roots
        .iter()
        .filter_map(|(root, dir)| Some((PathBuf::from(dir), holds(root)?)))
        .collect();
    v.extend(home.map(|h| (PathBuf::from(h), Disk::HOME)));
    v.extend(vault.map(|d| (d.to_path_buf(), Disk::VAULT)));
    v
}

/// This machine's drives as they are now: the ones holding its scanned
/// folders, home folder and Installer Vault (`vault`).
pub fn this_machine(machine: &MachineInfo, vault: Option<&Path>) -> Vec<Disk> {
    let home = crate::home_dir();
    measure(&places(machine, home.as_deref(), vault), crate::now())
}

/// What a logical root's folder holds, by its name.
fn holds(root: &str) -> Option<&'static str> {
    const PLUGIN_ROOTS: &[&str] = &["VST", "AU_", "AAX", "CLAP", "EXTRA_"];
    if PLUGIN_ROOTS.iter().any(|p| root.starts_with(p)) {
        Some(Disk::PLUGINS)
    } else if root.starts_with("PRESETS_") {
        Some(Disk::PRESETS)
    } else if root.starts_with("REAPER") || root.starts_with("ABLETON") {
        Some(Disk::DAW)
    } else {
        None
    }
}

/// Every local drive this machine has (not network shares, discs or
/// read-only images), each with what of `places` it holds, the one with the
/// home folder first. A place on a drive the OS didn't list (a network share
/// holding the vault) still gets its drive measured.
pub fn measure(places: &[(PathBuf, &'static str)], now: i64) -> Vec<Disk> {
    let startup = startup_disk();
    let places: Vec<(PathBuf, &'static str)> = places
        .iter()
        .filter_map(|(dir, what)| Some((std::fs::canonicalize(dir).ok()?, *what)))
        .collect();
    let (mut disks, elsewhere) = assign(&local_volumes(&startup), &places, now);
    for (dir, what) in elsewhere {
        let name = disk_name(&dir.to_string_lossy(), &startup);
        if let Some(d) = disks.iter_mut().find(|d| d.name == name) {
            add_holds(d, what);
            continue;
        }
        let Ok(stats) = fs4::statvfs(dir) else {
            continue;
        };
        if stats.total_space() == 0 {
            continue;
        }
        let mut d = Disk {
            name,
            label: None,
            total: stats.total_space(),
            free: stats.available_space().min(stats.total_space()),
            holds: Vec::new(),
            measured_at: now,
        };
        add_holds(&mut d, what);
        disks.push(d);
    }
    disks.sort_by_key(|d| (!d.holds.iter().any(|h| h == Disk::HOME), d.name.clone()));
    disks
}

/// A drive as the OS lists it, before Patch Pilot looks at what's on it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Volume {
    name: String,
    label: Option<String>,
    /// Where it's mounted: `C:\`, `/`, `/Volumes/Samples SSD`.
    mount: String,
    total: u64,
    free: u64,
}

/// The local drives the OS lists: Windows' fixed and removable drives,
/// the Mac's volumes Finder shows. Read-only ones (installer disk images,
/// discs) are left out, except a Mac's sealed system volume, which stands
/// for the startup disk.
fn local_volumes(startup: &str) -> Vec<Volume> {
    use sysinfo::{DiskRefreshKind, Disks};
    let disks = Disks::new_with_refreshed_list_specifics(DiskRefreshKind::nothing().with_storage());
    let mut out: Vec<Volume> = Vec::new();
    for d in disks.list() {
        let mount = d.mount_point().to_string_lossy().into_owned();
        let label = d.name().to_string_lossy().trim().to_owned();
        if let Some(v) = volume(&mount, &label, d.is_read_only(), startup) {
            if d.total_space() == 0 || out.iter().any(|o| o.name == v.name) {
                continue;
            }
            out.push(Volume {
                total: d.total_space(),
                free: d.available_space().min(d.total_space()),
                ..v
            });
        }
    }
    out
}

/// Name a listed volume, or `None` to leave it out.
fn volume(mount: &str, label: &str, read_only: bool, startup: &str) -> Option<Volume> {
    // The Mac's hidden system volumes (Data, Preboot, VM) are the startup disk.
    if mount.starts_with("/System/Volumes/") || (read_only && mount != "/") {
        return None;
    }
    let (name, label) = match windows_drive(mount) {
        // A drive mounted in a folder goes by the folder.
        Some(_) if mount.trim_end_matches('\\').len() > 2 => {
            (mount.trim_end_matches('\\').to_owned(), None)
        }
        Some(letter) => (letter, (!label.is_empty()).then(|| label.to_owned())),
        None if mount == "/" => (
            if label.is_empty() {
                startup.to_owned()
            } else {
                label.to_owned()
            },
            None,
        ),
        None => (disk_name(mount, startup), None),
    };
    Some(Volume {
        name,
        label,
        mount: mount.to_owned(),
        total: 0,
        free: 0,
    })
}

/// Every volume as a disk, holding the places under its mount point (the
/// deepest mount point wins), and the places on none of them.
fn assign<'a>(
    volumes: &[Volume],
    places: &'a [(PathBuf, &'static str)],
    now: i64,
) -> (Vec<Disk>, Vec<(&'a Path, &'static str)>) {
    let mut disks: Vec<Disk> = volumes
        .iter()
        .map(|v| Disk {
            name: v.name.clone(),
            label: v.label.clone(),
            total: v.total,
            free: v.free,
            holds: Vec::new(),
            measured_at: now,
        })
        .collect();
    let mut elsewhere = Vec::new();
    for (dir, what) in places {
        let path = dir.to_string_lossy();
        let best = volumes
            .iter()
            .enumerate()
            .filter(|(_, v)| under(&path, &v.mount))
            .max_by_key(|(_, v)| v.mount.len());
        match best {
            Some((i, _)) => add_holds(&mut disks[i], what),
            None => elsewhere.push((dir.as_path(), *what)),
        }
    }
    (disks, elsewhere)
}

/// Whether the path `dir` is at or below the mount point `mount`, ignoring
/// case and Windows' `\\?\` prefix.
fn under(dir: &str, mount: &str) -> bool {
    let dir = dir
        .strip_prefix(r"\\?\")
        .unwrap_or(dir)
        .to_ascii_lowercase();
    let mount = mount.to_ascii_lowercase();
    let base = mount.trim_end_matches(['/', '\\']);
    if base.is_empty() {
        return dir.starts_with('/');
    }
    dir == base
        || dir
            .strip_prefix(base)
            .is_some_and(|rest| rest.starts_with(['/', '\\']))
}

fn add_holds(d: &mut Disk, what: &str) {
    const ORDER: &[&str] = &[
        Disk::PLUGINS,
        Disk::PRESETS,
        Disk::DAW,
        Disk::HOME,
        Disk::VAULT,
    ];
    if !d.holds.iter().any(|h| h == what) {
        d.holds.push(what.to_owned());
        d.holds
            .sort_by_key(|h| ORDER.iter().position(|o| o == h).unwrap_or(ORDER.len()));
    }
}

/// True when `new` is worth storing and sending over `old`: other drives,
/// other contents, free space moved by a gigabyte, or `old` is a day old.
pub fn moved(old: &[Disk], new: &[Disk]) -> bool {
    if old.len() != new.len() {
        return true;
    }
    old.iter().zip(new).any(|(a, b)| {
        a.name != b.name
            || a.label != b.label
            || a.holds != b.holds
            || a.total != b.total
            || a.free.abs_diff(b.free) >= MOVED_BYTES
            || b.measured_at - a.measured_at >= STALE_SECS
    })
}

/// `Err` with a sentence for the user when `bytes` won't fit on the drive
/// holding `dir` (with a little room to spare). Fits when it can't tell.
pub fn room_for(dir: &Path, bytes: u64) -> Result<(), String> {
    let Some(at) = dir.ancestors().find(|d| d.exists()) else {
        return Ok(());
    };
    let Ok(stats) = fs4::statvfs(at) else {
        return Ok(());
    };
    let free = stats.available_space();
    if bytes.saturating_add(HEADROOM) <= free {
        return Ok(());
    }
    let name = std::fs::canonicalize(at)
        .map(|d| disk_name(&d.to_string_lossy(), &startup_disk()))
        .unwrap_or_else(|_| "this drive".to_owned());
    Err(format!(
        "Not enough space on {name}: this needs {} and {} is free. Free up some space, then try again.",
        size_label(bytes),
        size_label(free)
    ))
}

/// "412 GB", "1.2 TB", "640 MB", in the units this OS's own drive bars use:
/// Explorer counts in 1024s, Finder in 1000s.
pub fn size_label(bytes: u64) -> String {
    size_in(bytes, if cfg!(windows) { 1024.0 } else { 1000.0 })
}

fn size_in(bytes: u64, k: f64) -> String {
    let b = bytes as f64;
    for (unit, size) in [("TB", k * k * k * k), ("GB", k * k * k), ("MB", k * k)] {
        if b >= size {
            let n = b / size;
            return if n < 10.0 {
                format!("{:.1} {unit}", (n * 10.0).floor() / 10.0)
            } else {
                format!("{} {unit}", n.floor())
            };
        }
    }
    format!("{} KB", (b / k).ceil())
}

/// How the OS names the drive a canonical path is on: `C:` (or
/// `\\server\share`) on Windows; on a Mac the volume under `/Volumes`, else
/// the startup disk's name.
fn disk_name(path: &str, startup: &str) -> String {
    if let Some(name) = windows_drive(path) {
        return name;
    }
    match path.strip_prefix("/Volumes/") {
        Some(rest) => rest.split('/').next().unwrap_or(rest).to_owned(),
        None => startup.to_owned(),
    }
}

fn windows_drive(path: &str) -> Option<String> {
    let p = path.strip_prefix(r"\\?\").unwrap_or(path);
    if let Some(unc) = p
        .strip_prefix(r"UNC\")
        .or_else(|| (path.starts_with(r"\\") && !path.starts_with(r"\\?\")).then(|| &path[2..]))
    {
        let mut parts = unc.split('\\');
        let (server, share) = (parts.next()?, parts.next()?);
        return Some(format!(r"\\{server}\{share}"));
    }
    let mut chars = p.chars();
    match (chars.next(), chars.next()) {
        (Some(letter), Some(':')) if letter.is_ascii_alphabetic() => {
            Some(format!("{}:", letter.to_ascii_uppercase()))
        }
        _ => None,
    }
}

/// The Mac's startup disk as Finder names it: the `/Volumes` entry that links
/// back to `/`.
fn startup_disk() -> String {
    if cfg!(target_os = "macos")
        && let Ok(entries) = std::fs::read_dir("/Volumes")
    {
        for e in entries.flatten() {
            if std::fs::read_link(e.path()).is_ok_and(|t| t == Path::new("/")) {
                return e.file_name().to_string_lossy().into_owned();
            }
        }
    }
    if cfg!(target_os = "macos") {
        "Macintosh HD".to_owned()
    } else {
        "/".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patchpilot_core::{Arch, Os, PathMap};

    #[test]
    fn names_drives_the_way_the_os_shows_them() {
        assert_eq!(
            disk_name(r"\\?\C:\Program Files\Common Files\VST3", "/"),
            "C:"
        );
        assert_eq!(disk_name(r"d:\Samples", "/"), "D:");
        assert_eq!(disk_name(r"\\?\UNC\nas\audio\Vault", "/"), r"\\nas\audio");
        assert_eq!(disk_name(r"\\nas\audio\Vault", "/"), r"\\nas\audio");
        assert_eq!(
            disk_name("/Library/Audio/Plug-Ins/VST3", "Macintosh HD"),
            "Macintosh HD"
        );
        assert_eq!(
            disk_name("/Volumes/Samples SSD/Kontakt", "Macintosh HD"),
            "Samples SSD"
        );
        assert_eq!(
            disk_name("/Volumes/Samples SSD", "Macintosh HD"),
            "Samples SSD"
        );
    }

    #[test]
    fn every_root_counts_by_what_it_holds() {
        let machine = MachineInfo {
            id: "m".into(),
            name: "Studio".into(),
            os: Os::Windows,
            arch: Arch::X86_64,
            os_version: None,
            path_map: PathMap::new(Os::Windows)
                .with_root("VST3_SYSTEM", "C:/vst3")
                .with_root("EXTRA_1", "D:/plugins")
                .with_root("PRESETS_FABFILTER", "C:/presets")
                .with_root("REAPER_RESOURCES", "C:/reaper")
                .with_root("ABLETON_USER_LIBRARY", "C:/live")
                .with_root("SOMETHING_ELSE", "C:/x"),
            disks: Vec::new(),
        };
        let got = places(&machine, Some("C:/Users/me"), Some(Path::new("E:/Vault")));
        let holds: Vec<(&str, &str)> = got.iter().map(|(p, h)| (p.to_str().unwrap(), *h)).collect();
        assert_eq!(
            holds,
            [
                ("C:/live", Disk::DAW),
                ("D:/plugins", Disk::PLUGINS),
                ("C:/presets", Disk::PRESETS),
                ("C:/reaper", Disk::DAW),
                ("C:/vst3", Disk::PLUGINS),
                ("C:/Users/me", Disk::HOME),
                ("E:/Vault", Disk::VAULT),
            ]
        );
    }

    #[test]
    fn measures_each_drive_once_with_everything_on_it() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a"), dir.path().join("b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        let places = [
            (b.clone(), Disk::VAULT),
            (a.clone(), Disk::PLUGINS),
            (dir.path().join("gone"), Disk::PRESETS),
            (a, Disk::HOME),
        ];
        // Every local drive of the machine running the test, the temp
        // folder's holding all three, listed first for the home folder.
        let disks = measure(&places, 42);
        let holding: Vec<&Disk> = disks.iter().filter(|d| !d.holds.is_empty()).collect();
        assert_eq!(holding.len(), 1, "{disks:?}");
        assert_eq!(holding[0].holds, [Disk::PLUGINS, Disk::HOME, Disk::VAULT]);
        assert_eq!(disks[0], *holding[0]);
        assert!(
            disks
                .iter()
                .all(|d| d.total > 0 && d.free <= d.total && d.measured_at == 42)
        );
        assert!(room_for(&b, 1).is_ok());
        let err = room_for(&b.join("not/yet"), u64::MAX / 2).unwrap_err();
        assert!(err.starts_with("Not enough space on "), "{err}");
    }

    #[test]
    fn lists_the_drives_people_see() {
        let v = |mount: &str, label: &str, ro: bool| {
            volume(mount, label, ro, "Macintosh HD").map(|v| (v.name, v.label))
        };
        assert_eq!(v(r"C:\", "", false), Some(("C:".into(), None)));
        assert_eq!(
            v(r"D:\", "Samples", false),
            Some(("D:".into(), Some("Samples".into())))
        );
        assert_eq!(
            v(r"C:\Mounts\Libraries\", "Libs", false),
            Some((r"C:\Mounts\Libraries".into(), None))
        );
        assert_eq!(v(r"E:\", "DVD", true), None);
        // A Mac: the sealed system volume stands for the startup disk; its
        // hidden siblings and mounted installer images don't show.
        assert_eq!(
            v("/", "Macintosh HD", true),
            Some(("Macintosh HD".into(), None))
        );
        assert_eq!(v("/", "", true), Some(("Macintosh HD".into(), None)));
        assert_eq!(
            v("/System/Volumes/Data", "Macintosh HD - Data", false),
            None
        );
        assert_eq!(
            v("/Volumes/Samples SSD", "Samples SSD", false),
            Some(("Samples SSD".into(), None))
        );
        assert_eq!(v("/Volumes/Serum Installer", "Serum Installer", true), None);
    }

    #[test]
    fn places_land_on_the_drive_that_holds_them() {
        let vol = |name: &str, mount: &str| Volume {
            name: name.into(),
            label: None,
            mount: mount.into(),
            total: 1000,
            free: 500,
        };
        let volumes = [
            vol("C:", r"C:\"),
            vol("D:", r"D:\"),
            vol("Libs", r"D:\Libs\"),
            vol("Mac", "/"),
            vol("Ext", "/Volumes/Ext"),
        ];
        let places = [
            (
                PathBuf::from(r"\\?\C:\Program Files\Common Files\VST3"),
                Disk::PLUGINS,
            ),
            (PathBuf::from(r"\\?\d:\Libs\Kontakt"), Disk::VAULT),
            (PathBuf::from(r"D:\Libsy"), Disk::PRESETS),
            (PathBuf::from("/Volumes/Ext/Presets"), Disk::PRESETS),
            (PathBuf::from("/Users/me"), Disk::HOME),
            (PathBuf::from(r"\\?\UNC\nas\audio\Vault"), Disk::VAULT),
        ];
        let (disks, elsewhere) = assign(&volumes, &places, 7);
        let holds: Vec<(&str, Vec<&str>)> = disks
            .iter()
            .map(|d| {
                (
                    d.name.as_str(),
                    d.holds.iter().map(String::as_str).collect(),
                )
            })
            .collect();
        assert_eq!(
            holds,
            [
                ("C:", vec![Disk::PLUGINS]),
                ("D:", vec![Disk::PRESETS]),
                ("Libs", vec![Disk::VAULT]),
                ("Mac", vec![Disk::HOME]),
                ("Ext", vec![Disk::PRESETS]),
            ]
        );
        assert_eq!(
            elsewhere,
            [(Path::new(r"\\?\UNC\nas\audio\Vault"), Disk::VAULT)]
        );
    }

    #[test]
    fn only_a_real_move_is_worth_sending() {
        let d = |free: u64, at: i64| Disk {
            name: "C:".into(),
            label: None,
            total: 2_000_000_000_000,
            free,
            holds: vec![Disk::PLUGINS.into()],
            measured_at: at,
        };
        let old = [d(500_000_000_000, 0)];
        assert!(!moved(&old, &[d(500_200_000_000, 60)]));
        assert!(moved(&old, &[d(498_000_000_000, 60)]));
        assert!(moved(&old, &[d(500_000_000_000, STALE_SECS)]));
        assert!(moved(&[], &old));
        let mut more = d(500_000_000_000, 60);
        more.holds.push(Disk::HOME.into());
        assert!(moved(&old, &[more]));
    }

    #[test]
    fn sizes_read_like_a_drive_bar() {
        assert_eq!(size_in(412_345_000_000, 1000.0), "412 GB");
        assert_eq!(size_in(1_234_000_000_000, 1000.0), "1.2 TB");
        assert_eq!(size_in(9_990_000_000, 1000.0), "9.9 GB");
        assert_eq!(size_in(640_000_000, 1000.0), "640 MB");
        assert_eq!(size_in(1_500, 1000.0), "2 KB");
        assert_eq!(size_in(500 << 30, 1024.0), "500 GB");
        assert_eq!(size_in(2 << 40, 1024.0), "2.0 TB");
    }
}
