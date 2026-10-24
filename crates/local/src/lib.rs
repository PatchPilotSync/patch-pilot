//! This machine (#40): where its plug-ins and app data are, and scanning it.
//!
//! The glue between the OS and the other crates, shared by the `patchpilot`
//! CLI and the desktop app so both scan the same way. Read-only except for
//! Patch Pilot's own app-data folder.

mod anonymize;
pub mod apps;
pub mod sync;
mod watch;

use std::path::{Path, PathBuf};

use patchpilot_core::{Arch, ItemKind, Manifest, Os, PluginFormat};
use patchpilot_scanner::{PluginRoot, ScanInput, SystemDirs, default_roots};
use patchpilot_store::{SnapshotInfo, SnapshotReason, Store, StoreError};

pub use anonymize::anonymize;
pub use watch::{FolderWatch, watch_folders, watched_folders};

/// What to scan besides the defaults.
#[derive(Debug, Clone)]
pub struct ScanOptions {
    /// Machine ID, from [`patchpilot_store::load_or_create_machine_id`] for a
    /// saved scan.
    pub id: String,
    pub name: String,
    /// Standard plug-in folders and Reaper folder for this OS.
    pub default_folders: bool,
    pub extra_roots: Vec<PluginRoot>,
    /// Vendor apps and iLok.
    pub vendor_apps: bool,
    /// Reaper resource folder; `None` uses the standard one when
    /// `default_folders` is set.
    pub reaper_dir: Option<PathBuf>,
    /// Ableton User Library and Live preferences folder; `None` finds the
    /// standard ones when `default_folders` is set.
    pub ableton: Option<AbletonDirs>,
}

/// Where Ableton Live keeps what Patch Pilot shares. Either may be missing.
#[derive(Debug, Clone, Default)]
pub struct AbletonDirs {
    pub user_library: Option<PathBuf>,
    /// The newest `Live x.y.z` preferences folder.
    pub prefs: Option<PathBuf>,
}

impl ScanOptions {
    /// Everything standard on this machine.
    pub fn standard(id: String, name: String) -> Self {
        Self {
            id,
            name,
            default_folders: true,
            extra_roots: Vec::new(),
            vendor_apps: true,
            reaper_dir: None,
            ableton: None,
        }
    }
}

/// A step of [`scan_this_machine`], for progress UIs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    VendorApps,
    Plugins,
    Templates,
}

impl Stage {
    pub fn label(self) -> &'static str {
        match self {
            Self::VendorApps => "Checking vendor apps and licensing tools",
            Self::Plugins => "Reading plug-in folders",
            Self::Templates => "Reading Reaper and Ableton templates and settings",
        }
    }
}

#[derive(Debug)]
pub struct LocalScan {
    pub manifest: Manifest,
    pub warnings: Vec<String>,
    /// Reaper templates, project templates and FX chains found.
    pub templates: usize,
    /// Ableton settings, templates and User Library files found.
    pub ableton: usize,
}

impl LocalScan {
    pub fn plugins(&self) -> usize {
        self.manifest
            .items
            .iter()
            .filter(|i| i.kind == ItemKind::Plugin)
            .count()
    }
}

/// Scan this machine. `stage` is called before each step.
pub fn scan_this_machine(
    opts: ScanOptions,
    mut stage: impl FnMut(Stage),
) -> Result<LocalScan, String> {
    let os = Os::current().ok_or("scanning is supported on Windows and macOS only")?;
    let dirs = SystemDirs::from_env().ok_or("unsupported OS")?;

    let mut roots = if opts.default_folders {
        default_roots(&dirs)
    } else {
        Vec::new()
    };
    roots.extend(opts.extra_roots);
    let reaper_dir = opts.reaper_dir.or_else(|| {
        opts.default_folders
            .then(|| default_reaper_dir(os, &dirs))?
    });
    let ableton = opts.ableton.or_else(|| {
        opts.default_folders
            .then(|| default_ableton_dirs(os, &dirs))
    });

    stage(Stage::VendorApps);
    let vendor_apps = if opts.vendor_apps {
        detect_vendor_apps(os, &dirs)
    } else {
        Vec::new()
    };

    stage(Stage::Plugins);
    let output = patchpilot_scanner::scan_machine(ScanInput {
        id: opts.id,
        name: opts.name,
        os,
        arch: current_arch(),
        os_version: None,
        roots,
        vendor_apps,
        clap_cache: reaper_dir
            .as_deref()
            .map(patchpilot_scanner::ClapCache::load_reaper)
            .unwrap_or_default(),
        now: now(),
    });
    let mut manifest = output.manifest;

    stage(Stage::Templates);
    let templates = match &reaper_dir {
        Some(dir) => add_templates(&mut manifest, dir),
        None => 0,
    };
    let ableton = ableton.map_or(0, |dirs| add_ableton(&mut manifest, &dirs));
    Ok(LocalScan {
        manifest,
        warnings: output.warnings,
        templates,
        ableton,
    })
}

/// Save a scan as the machine's next snapshot (`first_scan`, then `scan`).
pub fn save_scan(store: &mut Store, manifest: &Manifest) -> Result<SnapshotInfo, StoreError> {
    let first = store.snapshots(&manifest.machine.id)?.is_empty();
    let reason = if first {
        SnapshotReason::FirstScan
    } else {
        SnapshotReason::Scan
    };
    store.save_manifest(manifest, reason, now())
}

/// Parse `FORMAT=PATH` (vst2, vst3, au, aax, clap) into the `n`th extra root.
pub fn extra_root(n: usize, spec: &str) -> Result<PluginRoot, String> {
    let (format, path) = spec
        .split_once('=')
        .ok_or_else(|| format!("{spec:?}: expected FORMAT=PATH"))?;
    let format = match format.to_ascii_lowercase().as_str() {
        "vst2" => PluginFormat::Vst2,
        "vst3" => PluginFormat::Vst3,
        "au" => PluginFormat::Au,
        "aax" => PluginFormat::Aax,
        "clap" => PluginFormat::Clap,
        _ => return Err(format!("{spec:?}: unknown format {format:?}")),
    };
    Ok(PluginRoot::new(format!("EXTRA_{n}"), format, path))
}

/// `$PATCHPILOT_DB`, else the app-data default.
pub fn default_db_path() -> Result<PathBuf, String> {
    if let Some(p) = std::env::var_os("PATCHPILOT_DB").filter(|p| !p.is_empty()) {
        return Ok(PathBuf::from(p));
    }
    let os = Os::current().ok_or("no default database on this OS")?;
    let local = std::env::var("LOCALAPPDATA").unwrap_or_default();
    patchpilot_store::default_db_path(os, &local, &home_dir().unwrap_or_default())
        .ok_or_else(|| "can't find the app-data folder".to_owned())
}

/// Marker file in the app-data folder: Patch Pilot has already turned on
/// "Open at Login" once, so it never does it again behind the user's back.
const LOGIN_DEFAULT_FILE: &str = "login-default-applied";

/// True once [`mark_login_default_applied`] has run for this app-data folder.
pub fn login_default_applied(app_dir: &Path) -> bool {
    app_dir.join(LOGIN_DEFAULT_FILE).exists()
}

/// Remember that the launch-at-login default was applied.
pub fn mark_login_default_applied(app_dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(app_dir)?;
    std::fs::write(app_dir.join(LOGIN_DEFAULT_FILE), b"")
}

/// The folder holding the database, where `machine-id` lives too.
pub fn app_dir(db: &Path) -> &Path {
    db.parent().unwrap_or(Path::new("."))
}

/// Strip home folder and username from paths before a manifest is shared.
pub fn anonymize_for_sharing(m: &mut Manifest) {
    let home = home_dir();
    let user = std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .ok();
    anonymize(m, home.as_deref(), user.as_deref());
}

#[cfg(windows)]
fn detect_vendor_apps(_os: Os, _dirs: &SystemDirs) -> Vec<patchpilot_scanner::VendorAppStatus> {
    patchpilot_scanner::detect_windows(&patchpilot_scanner::read_uninstall_entries())
}

#[cfg(not(windows))]
fn detect_vendor_apps(os: Os, dirs: &SystemDirs) -> Vec<patchpilot_scanner::VendorAppStatus> {
    match os {
        Os::Macos => {
            let mut roots = vec![PathBuf::from("/")];
            if !dirs.home.is_empty() {
                roots.push(PathBuf::from(&dirs.home));
            }
            patchpilot_scanner::detect_macos(&roots)
        }
        Os::Windows => Vec::new(),
    }
}

/// Reaper's standard resource folder, if it exists. It holds the CLAP cache
/// (real CLAP IDs without loading plug-ins, #8) and the templates (#37).
fn default_reaper_dir(os: Os, dirs: &SystemDirs) -> Option<PathBuf> {
    let appdata = std::env::var("APPDATA").unwrap_or_default();
    patchpilot_parity::reaper::reaper_resource_dir(os, &appdata, &dirs.home).filter(|d| d.is_dir())
}

/// Add Reaper templates under `dir` as items that require their plug-ins.
/// Returns how many were added.
pub fn add_templates(manifest: &mut Manifest, dir: &Path) -> usize {
    use patchpilot_parity::reaper::{REAPER_RESOURCES, template_items};
    let (products, items) = template_items(dir, manifest.machine.os, |r| {
        // Stock plug-ins ship with Reaper; user JSFX aren't scanned yet.
        (!r.is_stock() && r.format != PluginFormat::Jsfx).then(|| {
            patchpilot_scanner::product_id(
                r.vendor.as_deref(),
                &patchpilot_scanner::normalize_name(&r.name).name,
            )
        })
    });
    if items.is_empty() {
        return 0;
    }
    manifest.machine.path_map.roots.insert(
        REAPER_RESOURCES.to_owned(),
        dir.to_string_lossy().into_owned(),
    );
    manifest.products.extend(products);
    let n = items.len();
    manifest.items.extend(items);
    n
}

/// Live's newest preferences folder and the User Library it points at
/// (else the standard one), each only if it exists.
fn default_ableton_dirs(os: Os, dirs: &SystemDirs) -> AbletonDirs {
    use patchpilot_parity::ableton::{
        default_user_library, latest_prefs_dir, prefs_base, user_library_from_cfg,
    };
    let appdata = std::env::var("APPDATA").unwrap_or_default();
    let prefs = prefs_base(os, &appdata, &dirs.home).and_then(|b| latest_prefs_dir(&b, os));
    let user_library = prefs
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p.join("Library.cfg")).ok())
        .and_then(|cfg| user_library_from_cfg(&cfg, os))
        .filter(|d| d.is_dir())
        .or_else(|| default_user_library(os, &dirs.home).filter(|d| d.is_dir()));
    AbletonDirs {
        user_library,
        prefs,
    }
}

/// Add Ableton's shareable files as items, and both folders to the path map
/// even when empty, so this machine can receive them. Returns how many
/// items were added.
pub fn add_ableton(manifest: &mut Manifest, dirs: &AbletonDirs) -> usize {
    use patchpilot_parity::ableton::{ABLETON_PREFS, ABLETON_USER_LIBRARY, settings_items};
    let user_library = dirs.user_library.as_deref().filter(|d| d.is_dir());
    let prefs = dirs.prefs.as_deref().filter(|d| d.is_dir());
    for (root, dir) in [(ABLETON_USER_LIBRARY, user_library), (ABLETON_PREFS, prefs)] {
        if let Some(dir) = dir {
            manifest
                .machine
                .path_map
                .roots
                .insert(root.to_owned(), dir.to_string_lossy().into_owned());
        }
    }
    let (products, items) = settings_items(user_library, prefs, manifest.machine.os);
    let n = items.len();
    manifest.products.extend(products);
    manifest.items.extend(items);
    n
}

pub fn home_dir() -> Option<String> {
    std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .ok()
        .filter(|h| !h.is_empty())
}

pub fn current_arch() -> Arch {
    if cfg!(target_arch = "aarch64") {
        Arch::Aarch64
    } else {
        Arch::X86_64
    }
}

/// Unix seconds.
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// A per-scan machine ID for file-only scans before any saved one.
pub fn random_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let seed = format!("{nanos}-{}-{:p}", std::process::id(), &nanos);
    blake3::hash(seed.as_bytes()).to_hex()[..16].to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_default_is_applied_once() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("PatchPilot");
        assert!(!login_default_applied(&app));
        mark_login_default_applied(&app).unwrap();
        assert!(login_default_applied(&app));
    }
}
