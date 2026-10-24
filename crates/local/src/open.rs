//! Handing things to the OS on this machine: opening an installer, a vendor
//! app or a web page, and showing a file in Finder or Explorer.
//!
//! Everything here runs only because the user clicked something, and only
//! asks the OS to open the file the way a double-click would. No installer
//! is ever run silently or with arguments (CLAUDE.md rule 7), and vendor apps
//! are only opened, never signed in to (rule 6).

use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use patchpilot_scanner::{ADAPTERS, Detection, VendorAdapter, VendorAppStatus};
use serde::Serialize;

/// Open a file as a double-click would: an installer starts (with the OS's
/// own security prompts), a disk image mounts.
pub fn open_path(path: &Path) -> io::Result<()> {
    if cfg!(target_os = "macos") {
        spawn(Command::new("/usr/bin/open").arg(path))
    } else if cfg!(windows) {
        spawn(Command::new("explorer.exe").arg(path))
    } else {
        Err(unsupported())
    }
}

/// Show a file selected in Finder / Explorer.
pub fn reveal(path: &Path) -> io::Result<()> {
    if cfg!(target_os = "macos") {
        spawn(Command::new("/usr/bin/open").arg("-R").arg(path))
    } else {
        reveal_windows(path)
    }
}

#[cfg(windows)]
fn reveal_windows(path: &Path) -> io::Result<()> {
    use std::os::windows::process::CommandExt;
    // Explorer wants `/select,"C:\path"` as one argument with the quotes
    // inside; Windows paths can't contain quotes.
    spawn(Command::new("explorer.exe").raw_arg(format!("/select,\"{}\"", path.display())))
}

#[cfg(not(windows))]
fn reveal_windows(_path: &Path) -> io::Result<()> {
    Err(unsupported())
}

/// Open an `https://` page in the default browser.
pub fn open_url(url: &str) -> io::Result<()> {
    if !url.starts_with("https://") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not an https link",
        ));
    }
    if cfg!(target_os = "macos") {
        spawn(Command::new("/usr/bin/open").arg(url))
    } else if cfg!(windows) {
        spawn(Command::new("rundll32.exe").args(["url.dll,FileProtocolHandler", url]))
    } else {
        Err(unsupported())
    }
}

fn spawn(cmd: &mut Command) -> io::Result<()> {
    // Don't wait: the opened app runs on its own.
    cmd.spawn().map(drop)
}

fn unsupported() -> io::Error {
    io::Error::new(io::ErrorKind::Unsupported, "only on Windows and macOS")
}

/// A vendor app or licensing tool as this machine has it right now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VendorApp {
    /// Adapter ID, e.g. `native-access`.
    pub id: &'static str,
    pub name: &'static str,
    pub installed: bool,
    /// Patch Pilot can open it (installed, and the app itself was found).
    pub can_open: bool,
    /// Empty when there's no download page.
    pub download_url: &'static str,
}

/// Every known vendor app, with what can be done with it here. Reads the
/// registry (Windows) or app bundles (macOS) fresh each time.
pub fn vendor_apps() -> Vec<VendorApp> {
    detected()
        .into_iter()
        .map(|(status, target)| VendorApp {
            id: status.adapter.id,
            name: status.adapter.name,
            installed: matches!(status.detection, Detection::Detected { .. }),
            can_open: target.is_some(),
            download_url: status.adapter.download_url,
        })
        .collect()
}

/// What [`open_vendor_app`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Opened {
    App,
    /// It isn't installed, so its download page opened instead.
    DownloadPage,
}

/// Open a vendor app (e.g. Native Access, iLok License Manager) by adapter
/// ID, or its download page when it isn't installed here.
pub fn open_vendor_app(id: &str) -> io::Result<Opened> {
    let (status, target) = detected()
        .into_iter()
        .find(|(s, _)| s.adapter.id == id)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("unknown app {id:?}")))?;
    if let Some(app) = target {
        open_path(&app)?;
        return Ok(Opened::App);
    }
    let a = status.adapter;
    if a.download_url.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("{} has nothing to open", a.name),
        ));
    }
    open_url(a.download_url)?;
    Ok(Opened::DownloadPage)
}

/// Each adapter's status plus the path to open, if it can be opened.
fn detected() -> Vec<(VendorAppStatus, Option<PathBuf>)> {
    #[cfg(windows)]
    {
        let entries = patchpilot_scanner::read_uninstall_entries();
        patchpilot_scanner::detect_windows(&entries)
            .into_iter()
            .map(|s| {
                let exe = patchpilot_scanner::windows_executable(s.adapter, &entries);
                (s, exe)
            })
            .collect()
    }
    #[cfg(not(windows))]
    {
        let mut roots = vec![PathBuf::from("/")];
        if let Some(home) = crate::home_dir() {
            roots.push(PathBuf::from(home));
        }
        patchpilot_scanner::detect_macos(&roots)
            .into_iter()
            .map(|s| {
                let app = macos_app(&s);
                (s, app)
            })
            .collect()
    }
}

/// The `.app` bundle, if detection found one (PACE's helper isn't an app).
#[cfg_attr(windows, allow(dead_code))]
fn macos_app(s: &VendorAppStatus) -> Option<PathBuf> {
    let Detection::Detected {
        location: Some(l), ..
    } = &s.detection
    else {
        return None;
    };
    let p = PathBuf::from(l);
    (p.extension().is_some_and(|e| e == "app") && p.is_dir()).then_some(p)
}

/// The adapter for an ID.
pub fn adapter(id: &str) -> Option<&'static VendorAdapter> {
    ADAPTERS.iter().find(|a| a.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_https_links_open() {
        assert!(open_url("file:///etc/passwd").is_err());
        assert!(open_url("javascript:alert(1)").is_err());
    }

    #[test]
    fn every_app_has_a_download_page() {
        for a in ADAPTERS {
            assert!(a.download_url.starts_with("https://"), "{}", a.id);
        }
    }

    #[test]
    fn only_app_bundles_open_on_macos() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("Native Access.app");
        std::fs::create_dir_all(&app).unwrap();
        let helper = dir.path().join("com.paceap.eden.licensed");
        std::fs::write(&helper, b"").unwrap();
        let status = |p: &Path| VendorAppStatus {
            adapter: &ADAPTERS[0],
            detection: Detection::Detected {
                version: None,
                location: Some(p.to_string_lossy().into_owned()),
            },
        };
        assert_eq!(macos_app(&status(&app)), Some(app.clone()));
        assert_eq!(macos_app(&status(&helper)), None);
    }
}
