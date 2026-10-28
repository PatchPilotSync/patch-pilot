//! Content-addressed blob store for small user-owned files (CLAUDE.md rules
//! 2 and 4): the bytes a file had before Patch Pilot changed it.
//!
//! One file per blob at `<dir>/<first 2 hex>/<rest>`, named by BLAKE3, so
//! identical contents are stored once. Lives in app data, never in a user
//! folder.

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::{Result, StoreError};

/// Prefix of every blob hash.
const PREFIX: &str = "blake3:";

#[derive(Debug, Clone)]
pub struct BlobStore {
    dir: PathBuf,
}

impl BlobStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// `blake3:<hex>` of `bytes`, the key [`put`](Self::put) stores them under.
    pub fn hash(bytes: &[u8]) -> String {
        format!("{PREFIX}{}", blake3::hash(bytes).to_hex())
    }

    /// Store `bytes` (no-op if already stored) and return their hash.
    /// Written to a temp file, synced, then renamed into place.
    pub fn put(&self, bytes: &[u8]) -> Result<String> {
        let hash = Self::hash(bytes);
        let path = self.path(&hash)?;
        if path.is_file() {
            return Ok(hash);
        }
        let dir = path.parent().expect("blob path has a parent");
        std::fs::create_dir_all(dir).map_err(|e| io(dir, e))?;
        let tmp = path.with_extension("tmp");
        let write = || -> std::io::Result<()> {
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(bytes)?;
            f.sync_all()?;
            std::fs::rename(&tmp, &path)
        };
        write().map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            io(&path, e)
        })?;
        Ok(hash)
    }

    /// The bytes stored under `hash`, checked against it.
    pub fn get(&self, hash: &str) -> Result<Vec<u8>> {
        let path = self.path(hash)?;
        let bytes = std::fs::read(&path).map_err(|e| io(&path, e))?;
        if Self::hash(&bytes) != hash {
            return Err(StoreError::BadValue {
                column: "blob",
                value: hash.to_owned(),
            });
        }
        Ok(bytes)
    }

    pub fn contains(&self, hash: &str) -> bool {
        self.path(hash).is_ok_and(|p| p.is_file())
    }

    /// Remove blobs not in `keep` whose files are older than `min_age`, so a
    /// blob just written for a restore point not yet recorded stays. Returns
    /// the bytes freed.
    pub fn remove_unused(
        &self,
        keep: &std::collections::HashSet<String>,
        min_age: std::time::Duration,
    ) -> u64 {
        let mut freed = 0;
        let Ok(dirs) = std::fs::read_dir(&self.dir) else {
            return 0;
        };
        for dir in dirs.flatten() {
            let prefix = dir.file_name().to_string_lossy().into_owned();
            let Ok(files) = std::fs::read_dir(dir.path()) else {
                continue;
            };
            for file in files.flatten() {
                let hash = format!("{PREFIX}{prefix}{}", file.file_name().to_string_lossy());
                let Ok(meta) = file.metadata() else {
                    continue;
                };
                let old = meta
                    .modified()
                    .ok()
                    .and_then(|m| m.elapsed().ok())
                    .is_some_and(|age| age >= min_age);
                // Only real blobs: never a temp file or anything else in the folder.
                if old
                    && meta.is_file()
                    && !keep.contains(&hash)
                    && self.path(&hash).is_ok()
                    && std::fs::remove_file(file.path()).is_ok()
                {
                    freed += meta.len();
                }
            }
        }
        freed
    }

    fn path(&self, hash: &str) -> Result<PathBuf> {
        let hex = hash
            .strip_prefix(PREFIX)
            .filter(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit()))
            .ok_or_else(|| StoreError::BadValue {
                column: "blob hash",
                value: hash.to_owned(),
            })?;
        Ok(self.dir.join(&hex[..2]).join(&hex[2..]))
    }
}

fn io(path: &Path, source: std::io::Error) -> StoreError {
    StoreError::Io {
        path: path.to_owned(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_only_unused_blobs() {
        let dir = tempfile::tempdir().unwrap();
        let blobs = BlobStore::new(dir.path());
        let kept = blobs.put(b"kept").unwrap();
        let gone = blobs.put(b"gone").unwrap();
        std::fs::write(dir.path().join("notes.txt"), "not a blob").unwrap();
        let keep = std::collections::HashSet::from([kept.clone()]);
        // Too new to remove yet.
        assert_eq!(
            blobs.remove_unused(&keep, std::time::Duration::from_secs(3600)),
            0
        );
        assert_eq!(blobs.remove_unused(&keep, std::time::Duration::ZERO), 4);
        assert!(blobs.contains(&kept));
        assert!(!blobs.contains(&gone));
        assert!(dir.path().join("notes.txt").exists());
    }

    #[test]
    fn round_trip_and_dedupe() {
        let dir = tempfile::tempdir().unwrap();
        let blobs = BlobStore::new(dir.path());
        let a = blobs.put(b"<REAPER_TEMPLATE>").unwrap();
        assert_eq!(blobs.put(b"<REAPER_TEMPLATE>").unwrap(), a);
        assert!(blobs.contains(&a));
        assert_eq!(blobs.get(&a).unwrap(), b"<REAPER_TEMPLATE>");
        let files: Vec<_> = walk(dir.path());
        assert_eq!(files.len(), 1, "{files:?}");
    }

    #[test]
    fn rejects_bad_hashes_and_tampered_blobs() {
        let dir = tempfile::tempdir().unwrap();
        let blobs = BlobStore::new(dir.path());
        assert!(blobs.get("blake3:../../etc/passwd").is_err());
        assert!(!blobs.contains("nope"));
        let h = blobs.put(b"one").unwrap();
        std::fs::write(blobs.path(&h).unwrap(), b"two").unwrap();
        assert!(matches!(blobs.get(&h), Err(StoreError::BadValue { .. })));
    }

    fn walk(dir: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                out.extend(walk(&p));
            } else {
                out.push(p);
            }
        }
        out
    }
}
