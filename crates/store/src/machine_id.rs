use std::path::Path;

use crate::{Result, StoreError};

const MACHINE_ID_FILE: &str = "machine-id";

/// This machine's stable ID, read from `machine-id` in the app-data folder
/// `dir` (next to the database), or `None` if it was never created.
pub fn read_machine_id(dir: &Path) -> Result<Option<String>> {
    let path = dir.join(MACHINE_ID_FILE);
    match std::fs::read_to_string(&path) {
        Ok(s) => {
            let id = s.trim();
            if is_valid(id) {
                Ok(Some(id.to_owned()))
            } else {
                Err(StoreError::BadValue {
                    column: "machine-id",
                    value: id.to_owned(),
                })
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(StoreError::Io { path, source }),
    }
}

/// This machine's stable ID, creating `machine-id` in `dir` on first use.
///
/// The ID is random, not derived from hardware, and lives only in app-data
/// (CLAUDE.md rule 3). The file is written to a temp name and renamed, so a
/// crash never leaves a half-written ID.
pub fn load_or_create_machine_id(dir: &Path) -> Result<String> {
    if let Some(id) = read_machine_id(dir)? {
        return Ok(id);
    }
    let io = |path: &Path| {
        let path = path.to_owned();
        move |source| StoreError::Io { path, source }
    };
    std::fs::create_dir_all(dir).map_err(io(dir))?;
    let id = new_id();
    let tmp = dir.join(format!("{MACHINE_ID_FILE}.tmp"));
    let path = dir.join(MACHINE_ID_FILE);
    std::fs::write(&tmp, format!("{id}\n")).map_err(io(&tmp))?;
    std::fs::rename(&tmp, &path).map_err(io(&path))?;
    Ok(id)
}

fn is_valid(id: &str) -> bool {
    id.len() == 16 && id.bytes().all(|b| b.is_ascii_hexdigit())
}

/// 16 hex chars from the clock, process ID and an address. Unique per
/// install is all we need; it is not a secret.
fn new_id() -> String {
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
    fn created_once_then_reused() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("PatchPilot");
        assert_eq!(read_machine_id(&app).unwrap(), None);
        let id = load_or_create_machine_id(&app).unwrap();
        assert!(is_valid(&id), "{id}");
        assert_eq!(load_or_create_machine_id(&app).unwrap(), id);
        assert_eq!(read_machine_id(&app).unwrap(), Some(id));
        assert!(!app.join("machine-id.tmp").exists());
    }

    #[test]
    fn rejects_a_damaged_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(MACHINE_ID_FILE), "not an id").unwrap();
        assert!(matches!(
            load_or_create_machine_id(dir.path()),
            Err(StoreError::BadValue { .. })
        ));
    }
}
