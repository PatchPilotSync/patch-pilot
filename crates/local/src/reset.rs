//! Start over on this machine (Settings, decision #97): Patch Pilot forgets
//! this machine's scans, links and settings, as on a new install. Nothing is
//! deleted. The store can't move while it's open, so a reset is asked for
//! with a marker and done at the next launch, before anything opens the
//! store: every file in app data moves into a `Before reset …` folder there.
//!
//! The launch after Start over can begin while the old process still holds
//! the store open (Windows won't rename an open file), so each move is tried
//! again for a while, the store goes first, and a reset that still can't
//! finish puts back what it moved (decision #99). Half a reset forgot the
//! machine id but kept its scans, so the next scan showed the machine twice.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const MARKER: &str = "reset-pending";
const KEPT_PREFIX: &str = "Before reset";
/// How long the launch waits for the old process to let go of its files.
const WAIT: Duration = Duration::from_secs(15);

/// Ask for a reset at the next launch.
pub fn request(app_dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(app_dir)
        .and_then(|()| std::fs::write(app_dir.join(MARKER), b""))
        .map_err(|e| format!("{}: {e}", app_dir.display()))
}

/// If a reset was asked for, move everything in the store's folder (except
/// earlier `Before reset` folders) into `Before reset <stamp>` and return
/// that folder. Called at launch, before the store opens. All or nothing: on
/// an error nothing has moved and the reset is tried again next launch.
pub fn finish(db: &Path, stamp: &str) -> Result<Option<PathBuf>, String> {
    finish_with(db, stamp, WAIT, |from, to| std::fs::rename(from, to))
}

fn finish_with(
    db: &Path,
    stamp: &str,
    wait: Duration,
    mut rename: impl FnMut(&Path, &Path) -> std::io::Result<()>,
) -> Result<Option<PathBuf>, String> {
    let app_dir = crate::app_dir(db);
    let marker = app_dir.join(MARKER);
    if !marker.exists() {
        return Ok(None);
    }
    let kept = app_dir.join(format!("{KEPT_PREFIX} {stamp}"));
    let io = |e: std::io::Error| format!("{}: {e}", kept.display());
    let store = db
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let mut names: Vec<_> = std::fs::read_dir(app_dir)
        .map_err(io)?
        .flatten()
        .map(|e| e.file_name())
        .filter(|n| !n.to_string_lossy().starts_with(KEPT_PREFIX) && n != MARKER)
        .collect();
    // The store and its -wal/-shm first: they're what the old process holds.
    names.sort_by_key(|n| !n.to_string_lossy().starts_with(&store));
    std::fs::create_dir_all(&kept).map_err(io)?;

    let deadline = Instant::now() + wait;
    let mut moved = Vec::new();
    for name in names {
        let (from, to) = (app_dir.join(&name), kept.join(&name));
        let result = loop {
            match rename(&from, &to) {
                Err(_) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(200));
                }
                r => break r,
            }
        };
        if let Err(e) = result {
            for name in moved.iter().rev() {
                let _ = rename(&kept.join(name), &app_dir.join(name));
            }
            let _ = std::fs::remove_dir(&kept);
            return Err(format!("{}: {e}", from.display()));
        }
        moved.push(name);
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
        let db = dir.join("patchpilot.db");
        std::fs::write(&db, "db").unwrap();
        std::fs::create_dir(dir.join("blobs")).unwrap();
        std::fs::write(dir.join("blobs").join("x"), "x").unwrap();
        assert_eq!(finish(&db, "1").unwrap(), None);
        assert!(db.exists());

        request(dir).unwrap();
        let kept = finish(&db, "2026-10-28 0410").unwrap().unwrap();
        assert!(kept.join("patchpilot.db").exists());
        assert!(kept.join("blobs").join("x").exists());
        assert!(!db.exists() && !dir.join(MARKER).exists());

        // A second reset keeps the first one's folder where it is.
        std::fs::write(&db, "db2").unwrap();
        request(dir).unwrap();
        let again = finish(&db, "later").unwrap().unwrap();
        assert!(again.join("patchpilot.db").exists());
        assert!(kept.exists());
        assert_eq!(std::fs::read_dir(dir).unwrap().count(), 2);
    }

    fn rig(dir: &Path) -> PathBuf {
        for name in [
            "machine-id",
            "patchpilot.db",
            "patchpilot.db-wal",
            "rig-key",
        ] {
            std::fs::write(dir.join(name), name).unwrap();
        }
        request(dir).unwrap();
        dir.join("patchpilot.db")
    }

    #[test]
    fn waits_for_the_old_process_to_let_go_of_the_store() {
        let tmp = tempfile::tempdir().unwrap();
        let db = rig(tmp.path());
        let mut order = Vec::new();
        let mut held = 3;
        let kept = finish_with(&db, "s", Duration::from_secs(5), |from, to| {
            let name = from.file_name().unwrap().to_string_lossy().into_owned();
            if name == "patchpilot.db" && held > 0 {
                held -= 1;
                return Err(std::io::Error::other("in use"));
            }
            order.push(name);
            std::fs::rename(from, to)
        })
        .unwrap()
        .unwrap();
        order[..2].sort();
        assert_eq!(order[..2], ["patchpilot.db", "patchpilot.db-wal"]);
        assert!(kept.join("machine-id").exists() && kept.join("patchpilot.db").exists());
        assert!(!tmp.path().join(MARKER).exists());
    }

    #[test]
    fn a_reset_that_cannot_finish_moves_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let db = rig(dir);
        let err = finish_with(&db, "s", Duration::ZERO, |from, to| {
            if from.ends_with("rig-key") {
                return Err(std::io::Error::other("in use"));
            }
            std::fs::rename(from, to)
        })
        .unwrap_err();
        assert!(err.contains("rig-key"), "{err}");
        // The machine keeps its id with its scans, and the reset is still asked for.
        for name in [
            "machine-id",
            "patchpilot.db",
            "patchpilot.db-wal",
            "rig-key",
            MARKER,
        ] {
            assert!(dir.join(name).exists(), "{name}");
        }
        assert!(!dir.join("Before reset s").exists());
    }
}
