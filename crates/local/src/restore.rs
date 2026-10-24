//! The one way Patch Pilot changes user files, and undoing it (rollback).
//!
//! [`apply_changes`] refuses while the DAW is open (CLAUDE.md rule 5), saves
//! every file it's about to touch to the blob store and records a restore
//! point (rule 4), then writes each file to a temp file beside it and renames
//! it into place. [`undo`] puts a restore point's files back the same way, so
//! an undo is itself a restore point and can be undone.

use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};

use patchpilot_store::{
    BlobStore, FileRecord, NewRestorePoint, RestoreKind, RestorePointRecord, Store, StoreError,
};

/// User files bigger than this aren't "small user-owned files" (rule 2).
const MAX_FILE: u64 = 64 * 1024 * 1024;

/// A DAW whose files a change belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Daw {
    Reaper,
    Ableton,
}

impl Daw {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Reaper => "reaper",
            Self::Ableton => "ableton",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "reaper" => Some(Self::Reaper),
            "ableton" => Some(Self::Ableton),
            _ => None,
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Reaper => "Reaper",
            Self::Ableton => "Ableton Live",
        }
    }
}

/// Write `contents` to `path`, or remove it when `None`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChange {
    pub path: PathBuf,
    pub contents: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeSet {
    pub machine_id: String,
    /// Shown in History, e.g. "Brought over 3 Reaper templates from Studio PC".
    pub label: String,
    /// Refuse while this DAW is open. `None` for files no DAW rewrites.
    pub daw: Option<Daw>,
    pub changes: Vec<FileChange>,
    pub now: i64,
}

/// What [`apply_changes`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestorePoint {
    /// `None` when every file already had the requested contents, so
    /// nothing was written or recorded.
    pub id: Option<i64>,
    /// Files actually written or removed.
    pub changed: Vec<PathBuf>,
}

#[derive(Debug, thiserror::Error)]
pub enum ApplyError {
    #[error(
        "Close {} first: Patch Pilot never changes its files while it's open.",
        .0.display_name()
    )]
    DawRunning(Daw),
    #[error("{0}: Patch Pilot only changes files at full, readable paths")]
    BadPath(PathBuf),
    #[error("{0}: listed twice in one change")]
    Duplicate(PathBuf),
    #[error("{0}: too big to keep a copy of; Patch Pilot only changes small files")]
    TooBig(PathBuf),
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("no restore point {0}")]
    UnknownRestorePoint(i64),
    #[error("the saved copy of {0} is missing, so it can't be put back")]
    MissingBackup(PathBuf),
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// `blobs` next to the database in app data.
pub fn default_blob_dir() -> Option<PathBuf> {
    let db = crate::default_db_path().ok()?;
    Some(crate::app_dir(&db).join("blobs"))
}

/// Save what `req` will change, record a restore point, then make the
/// changes. If a write fails, the files already written are put back and the
/// restore point is dropped.
pub fn apply_changes(
    store: &mut Store,
    blobs: &Path,
    req: &ChangeSet,
) -> Result<RestorePoint, ApplyError> {
    apply(store, blobs, req, RestoreKind::Change, None)
}

/// Put the files of restore point `id` back as they were before it. Recorded
/// as a new restore point, so the undo can be undone too.
pub fn undo(
    store: &mut Store,
    blobs: &Path,
    id: i64,
    now: i64,
) -> Result<RestorePoint, ApplyError> {
    let point = store
        .restore_point(id)?
        .ok_or(ApplyError::UnknownRestorePoint(id))?;
    let blob_store = BlobStore::new(blobs);
    let changes = point
        .files
        .iter()
        .map(|f| {
            let path = PathBuf::from(&f.path);
            let contents = match &f.before_hash {
                None => None,
                Some(h) => Some(
                    blob_store
                        .get(h)
                        .map_err(|_| ApplyError::MissingBackup(path.clone()))?,
                ),
            };
            Ok(FileChange { path, contents })
        })
        .collect::<Result<_, ApplyError>>()?;
    let req = ChangeSet {
        machine_id: point.machine_id.clone(),
        label: format!("Undid: {}", point.label),
        daw: point.daw.as_deref().and_then(Daw::parse),
        changes,
        now,
    };
    apply(store, blobs, &req, RestoreKind::Undo, Some(id))
}

/// What undoing a restore point would do to one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoFile {
    pub path: PathBuf,
    /// True: the file is put back. False: the change created it, so it's
    /// removed.
    pub restore: bool,
    /// The file is no longer what the change wrote (edited since, or changed
    /// by a later restore point). Undoing replaces it; that's saved first.
    pub changed_since: bool,
    /// The saved copy is gone from app data, so the undo would fail.
    pub missing_backup: bool,
}

/// Preview [`undo`] for a restore point, reading the files as they are now.
pub fn undo_preview(blobs: &Path, point: &RestorePointRecord) -> Vec<UndoFile> {
    let blob_store = BlobStore::new(blobs);
    point
        .files
        .iter()
        .map(|f| {
            let path = PathBuf::from(&f.path);
            let now = std::fs::read(&path).ok().map(|b| BlobStore::hash(&b));
            UndoFile {
                restore: f.before_hash.is_some(),
                changed_since: now != f.after_hash,
                missing_backup: f
                    .before_hash
                    .as_ref()
                    .is_some_and(|h| !blob_store.contains(h)),
                path,
            }
        })
        .collect()
}

/// Every file changed after scan `snapshot_id` was the latest, with the
/// state it had then: the earliest restore point after it that touched the
/// file has its `before_hash`, the latest has what's there now.
fn changed_since(
    store: &Store,
    machine_id: &str,
    snapshot_id: i64,
) -> Result<(Vec<FileRecord>, Vec<Daw>), ApplyError> {
    let mut files: Vec<FileRecord> = Vec::new();
    let mut daws = Vec::new();
    // Oldest first.
    for p in store.restore_points(machine_id)?.into_iter().rev() {
        if p.snapshot_id.unwrap_or(0) < snapshot_id {
            continue;
        }
        if let Some(d) = p.daw.as_deref().and_then(Daw::parse)
            && !daws.contains(&d)
        {
            daws.push(d);
        }
        for f in p.files {
            match files.iter_mut().find(|g| g.path == f.path) {
                Some(g) => g.after_hash = f.after_hash,
                None => files.push(f),
            }
        }
    }
    // Files that ended up as they started need nothing.
    files.retain(|f| f.before_hash != f.after_hash);
    Ok((files, daws))
}

/// Preview [`go_back`]: the files it would put back or remove.
pub fn go_back_preview(
    store: &Store,
    blobs: &Path,
    machine_id: &str,
    snapshot_id: i64,
) -> Result<Vec<UndoFile>, ApplyError> {
    let (files, _) = changed_since(store, machine_id, snapshot_id)?;
    let point = RestorePointRecord {
        id: 0,
        machine_id: machine_id.to_owned(),
        created_at: 0,
        kind: RestoreKind::Undo,
        label: String::new(),
        daw: None,
        snapshot_id: None,
        undoes: None,
        undone_by: None,
        files,
    };
    Ok(undo_preview(blobs, &point))
}

/// Put every file Patch Pilot changed since scan `snapshot_id` back as it
/// was then, as one restore point (so going back can be undone too).
/// Plug-ins aren't touched.
pub fn go_back(
    store: &mut Store,
    blobs: &Path,
    machine_id: &str,
    snapshot_id: i64,
    label: &str,
    now: i64,
) -> Result<RestorePoint, ApplyError> {
    let (files, daws) = changed_since(store, machine_id, snapshot_id)?;
    if let Some(&d) = daws.iter().find(|d| daw_running(**d)) {
        return Err(ApplyError::DawRunning(d));
    }
    let blob_store = BlobStore::new(blobs);
    let changes = files
        .iter()
        .map(|f| {
            let path = PathBuf::from(&f.path);
            let contents = match &f.before_hash {
                None => None,
                Some(h) => Some(
                    blob_store
                        .get(h)
                        .map_err(|_| ApplyError::MissingBackup(path.clone()))?,
                ),
            };
            Ok(FileChange { path, contents })
        })
        .collect::<Result<_, ApplyError>>()?;
    let req = ChangeSet {
        machine_id: machine_id.to_owned(),
        label: label.to_owned(),
        // Checked above; recorded so undoing this checks the same app.
        daw: daws.first().copied(),
        changes,
        now,
    };
    apply_checked(store, blobs, &req, RestoreKind::Undo, None)
}

fn apply(
    store: &mut Store,
    blobs: &Path,
    req: &ChangeSet,
    kind: RestoreKind,
    undoes: Option<i64>,
) -> Result<RestorePoint, ApplyError> {
    if let Some(daw) = req.daw
        && daw_running(daw)
    {
        return Err(ApplyError::DawRunning(daw));
    }
    apply_checked(store, blobs, req, kind, undoes)
}

/// [`apply`] once the DAW check has passed.
fn apply_checked(
    store: &mut Store,
    blobs: &Path,
    req: &ChangeSet,
    kind: RestoreKind,
    undoes: Option<i64>,
) -> Result<RestorePoint, ApplyError> {
    let mut seen = BTreeSet::new();
    for c in &req.changes {
        if !c.path.is_absolute() || c.path.to_str().is_none() || c.path.file_name().is_none() {
            return Err(ApplyError::BadPath(c.path.clone()));
        }
        if !seen.insert(&c.path) {
            return Err(ApplyError::Duplicate(c.path.clone()));
        }
    }

    // Save every file as it is now; skip the ones already as requested.
    let blob_store = BlobStore::new(blobs);
    let mut todo = Vec::new();
    for c in &req.changes {
        let before = read_current(&c.path)?;
        let before_hash = before.as_deref().map(BlobStore::hash);
        let after_hash = c.contents.as_deref().map(BlobStore::hash);
        if before_hash == after_hash {
            continue;
        }
        if let Some(bytes) = &before {
            blob_store.put(bytes)?;
        }
        todo.push((
            c,
            FileRecord {
                path: c.path.to_str().expect("checked above").to_owned(),
                before_hash,
                after_hash,
            },
            before,
        ));
    }
    if todo.is_empty() {
        return Ok(RestorePoint {
            id: None,
            changed: Vec::new(),
        });
    }

    let files: Vec<FileRecord> = todo.iter().map(|(_, f, _)| f.clone()).collect();
    let id = store.add_restore_point(&NewRestorePoint {
        machine_id: &req.machine_id,
        created_at: req.now,
        kind,
        label: &req.label,
        daw: req.daw.map(Daw::as_str),
        undoes,
        files: &files,
    })?;

    for (n, (c, _, _)) in todo.iter().enumerate() {
        if let Err(e) = write_atomic(&c.path, c.contents.as_deref()) {
            // Put back what was already written, newest first.
            for (c, _, before) in todo[..n].iter().rev() {
                let _ = write_atomic(&c.path, before.as_deref());
            }
            store.discard_restore_point(id)?;
            return Err(e);
        }
    }
    Ok(RestorePoint {
        id: Some(id),
        changed: todo.into_iter().map(|(c, _, _)| c.path.clone()).collect(),
    })
}

/// The file's bytes, `None` if it doesn't exist.
fn read_current(path: &Path) -> Result<Option<Vec<u8>>, ApplyError> {
    let io = |source| ApplyError::Io {
        path: path.to_owned(),
        source,
    };
    match std::fs::metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(io(e)),
        Ok(m) if m.is_dir() => {
            return Err(io(std::io::Error::other("is a folder, not a file")));
        }
        Ok(m) if m.len() > MAX_FILE => return Err(ApplyError::TooBig(path.to_owned())),
        Ok(_) => {}
    }
    std::fs::read(path).map(Some).map_err(io)
}

/// Write via a temp file in the same folder, synced, then renamed over the
/// target; or remove the file. Never leaves a second copy behind (rule 3).
fn write_atomic(path: &Path, contents: Option<&[u8]>) -> Result<(), ApplyError> {
    let io = |source| ApplyError::Io {
        path: path.to_owned(),
        source,
    };
    let Some(bytes) = contents else {
        return match std::fs::remove_file(path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(io(e)),
            _ => Ok(()),
        };
    };
    let dir = path
        .parent()
        .ok_or_else(|| ApplyError::BadPath(path.to_owned()))?;
    std::fs::create_dir_all(dir).map_err(io)?;
    let name = path
        .file_name()
        .expect("checked in apply")
        .to_string_lossy();
    let tmp = dir.join(format!(".{name}.patchpilot-tmp"));
    let write = || -> std::io::Result<()> {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)
    };
    write().map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        io(e)
    })
}

/// True if `daw` is open, or if the process list can't be read (when unsure,
/// don't touch its files).
pub fn daw_running(daw: Daw) -> bool {
    match process_names() {
        Some(names) => names.iter().any(|n| is_daw_process(daw, n)),
        None => true,
    }
}

/// Whether a process name or path (`tasklist` image name on Windows, `ps`
/// command path on macOS) is `daw`.
pub fn is_daw_process(daw: Daw, name: &str) -> bool {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let base_lower = base.to_ascii_lowercase();
    match daw {
        Daw::Reaper => base_lower == "reaper" || base_lower == "reaper.exe",
        Daw::Ableton => {
            (base_lower.starts_with("ableton live") && base_lower.ends_with(".exe"))
                || (base == "Live" && name.contains("Ableton Live"))
        }
    }
}

#[cfg(windows)]
fn process_names() -> Option<Vec<String>> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let out = std::process::Command::new("tasklist")
        .args(["/FO", "CSV", "/NH"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    // "reaper.exe","1234","Console","1","120,000 K"
    Some(
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|l| l.strip_prefix('"')?.split('"').next().map(str::to_owned))
            .collect(),
    )
}

#[cfg(not(windows))]
fn process_names() -> Option<Vec<String>> {
    let out = std::process::Command::new("ps")
        .args(["-axo", "comm="])
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    Some(
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(|l| l.trim().to_owned())
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_daw_processes() {
        assert!(is_daw_process(Daw::Reaper, "reaper.exe"));
        assert!(is_daw_process(Daw::Reaper, "REAPER.exe"));
        assert!(is_daw_process(
            Daw::Reaper,
            "/Applications/REAPER.app/Contents/MacOS/REAPER"
        ));
        assert!(!is_daw_process(Daw::Reaper, "reaper_host64.exe"));
        assert!(is_daw_process(Daw::Ableton, "Ableton Live 12 Suite.exe"));
        assert!(is_daw_process(
            Daw::Ableton,
            "/Applications/Ableton Live 12 Suite.app/Contents/MacOS/Live"
        ));
        assert!(!is_daw_process(Daw::Ableton, "Ableton Index.exe"));
        assert!(!is_daw_process(Daw::Ableton, "/usr/bin/Live"));
    }
}
