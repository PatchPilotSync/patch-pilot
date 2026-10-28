//! Start over on this machine (Settings, decision #97): Patch Pilot forgets
//! this machine's scans, links and settings, as on a new install. Nothing is
//! deleted. The store can't move while it's open, so a reset is asked for
//! with a marker and done at the next launch, before anything opens the
//! store: every file in app data moves into a `Before reset …` folder there.

use std::path::{Path, PathBuf};

const MARKER: &str = "reset-pending";
const KEPT_PREFIX: &str = "Before reset";

/// Ask for a reset at the next launch.
pub fn request(app_dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(app_dir)
        .and_then(|()| std::fs::write(app_dir.join(MARKER), b""))
        .map_err(|e| format!("{}: {e}", app_dir.display()))
}

/// If a reset was asked for, move everything in `app_dir` (except earlier
/// `Before reset` folders) into `Before reset <stamp>` and return that
/// folder. Called at launch, before the store opens.
pub fn finish(app_dir: &Path, stamp: &str) -> Result<Option<PathBuf>, String> {
    let marker = app_dir.join(MARKER);
    if !marker.exists() {
        return Ok(None);
    }
    let kept = app_dir.join(format!("{KEPT_PREFIX} {stamp}"));
    let io = |e: std::io::Error| format!("{}: {e}", kept.display());
    std::fs::create_dir_all(&kept).map_err(io)?;
    for entry in std::fs::read_dir(app_dir).map_err(io)?.flatten() {
        let name = entry.file_name();
        if name.to_string_lossy().starts_with(KEPT_PREFIX) || name == MARKER {
            continue;
        }
        std::fs::rename(entry.path(), kept.join(&name)).map_err(io)?;
    }
    // Last, so a reset cut short tries again at the next launch.
    std::fs::remove_file(&marker).map_err(io)?;
    Ok(Some(kept))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_everything_aside_once() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::write(dir.join("patchpilot.db"), "db").unwrap();
        std::fs::create_dir(dir.join("blobs")).unwrap();
        std::fs::write(dir.join("blobs").join("x"), "x").unwrap();
        assert_eq!(finish(dir, "1").unwrap(), None);
        assert!(dir.join("patchpilot.db").exists());

        request(dir).unwrap();
        let kept = finish(dir, "2026-10-28 0410").unwrap().unwrap();
        assert!(kept.join("patchpilot.db").exists());
        assert!(kept.join("blobs").join("x").exists());
        assert!(!dir.join("patchpilot.db").exists() && !dir.join(MARKER).exists());

        // A second reset keeps the first one's folder where it is.
        std::fs::write(dir.join("patchpilot.db"), "db2").unwrap();
        request(dir).unwrap();
        let again = finish(dir, "later").unwrap().unwrap();
        assert!(again.join("patchpilot.db").exists());
        assert!(kept.exists());
        assert_eq!(std::fs::read_dir(dir).unwrap().count(), 2);
    }
}
