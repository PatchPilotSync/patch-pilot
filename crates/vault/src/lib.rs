//! Installer Vault (strategy section 13): the plug-in installers a user
//! downloaded, kept on storage they choose (an external SSD, a NAS, their own
//! cloud folder), so a plug-in missing on another machine is one click from
//! its installer.
//!
//! The vault is a plain folder, readable without Patch Pilot:
//! `<Vendor>/<Product>/<version>/<os-arch>/<file>`, plus `vault.json`, the
//! index. Patch Pilot never hosts installers (decision #8), never runs one
//! without the user clicking (CLAUDE.md rule 7), and never writes `(1)`
//! copies: every file lands under a temporary name and is renamed into place.
//!
//! The index is JSON rather than SQLite because the folder may live on a NAS
//! or a synced drive, where SQLite's locking isn't safe, and because the user
//! can read it.

mod identify;
pub mod signature;

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use patchpilot_core::{Arch, Os, ProductId};
use patchpilot_parity::{VersionOrder, compare_versions};
use serde::{Deserialize, Serialize};

pub use identify::{Guess, INSTALLER_EXTENSIONS, compact, guess, is_installer, match_product};

/// The index file at the vault's root.
pub const INDEX_FILE: &str = "vault.json";
/// Readers refuse an index with a newer format.
pub const INDEX_VERSION: u32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    #[error("{0}: {1}")]
    Io(String, io::Error),
    #[error("{0}: not a vault index ({1})")]
    BadIndex(String, serde_json::Error),
    #[error("{0} was written by a newer Patch Pilot (index version {1}); update the app")]
    NewerIndex(String, u32),
    #[error("no installer {0} in the vault")]
    NoEntry(String),
}

fn io_err(path: &Path) -> impl FnOnce(io::Error) -> VaultError + '_ {
    move |e| VaultError::Io(path.display().to_string(), e)
}

/// One installer in the vault.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// First 16 hex characters of `hash`; stable, used by the app.
    pub id: String,
    /// BLAKE3 of the file, hex.
    pub hash: String,
    pub size: u64,
    /// Path from the vault root, `/`-separated on every OS.
    pub file: String,
    pub vendor: String,
    pub product: String,
    /// The product it installs, when it matched one Patch Pilot knows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub product_id: Option<ProductId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// `None` when the file doesn't say (some `.zip`s hold both).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os: Option<Os>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arch: Option<Arch>,
    /// The file's name when it was added.
    pub source_name: String,
    /// Who signed it (Developer ID / Authenticode subject), when it was
    /// signed and the OS could tell.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signer: Option<String>,
    /// Unix seconds.
    pub added_at: i64,
}

impl Entry {
    /// True if this installer is for `os` (or doesn't say).
    pub fn fits(&self, os: Os) -> bool {
        self.os.is_none_or(|o| o == os)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Index {
    pub version: u32,
    pub entries: Vec<Entry>,
}

impl Default for Index {
    fn default() -> Self {
        Self {
            version: INDEX_VERSION,
            entries: Vec::new(),
        }
    }
}

/// What the user confirmed about an installer before adding it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct NewEntry {
    pub vendor: String,
    pub product: String,
    pub product_id: Option<ProductId>,
    pub version: Option<String>,
    pub os: Option<Os>,
    pub arch: Option<Arch>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Added {
    New(Entry),
    /// The same file (by hash) was already in the vault.
    AlreadyThere(Entry),
}

impl Added {
    pub fn entry(&self) -> &Entry {
        match self {
            Self::New(e) | Self::AlreadyThere(e) => e,
        }
    }
}

/// A vault folder.
#[derive(Debug, Clone)]
pub struct Vault {
    pub root: PathBuf,
}

impl Vault {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The index; empty if the folder has none yet.
    pub fn index(&self) -> Result<Index, VaultError> {
        let path = self.root.join(INDEX_FILE);
        let text = match fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Index::default()),
            Err(e) => return Err(io_err(&path)(e)),
        };
        let index: Index = serde_json::from_str(&text)
            .map_err(|e| VaultError::BadIndex(path.display().to_string(), e))?;
        if index.version > INDEX_VERSION {
            return Err(VaultError::NewerIndex(
                path.display().to_string(),
                index.version,
            ));
        }
        Ok(index)
    }

    pub fn entry(&self, id: &str) -> Result<Entry, VaultError> {
        self.index()?
            .entries
            .into_iter()
            .find(|e| e.id == id)
            .ok_or_else(|| VaultError::NoEntry(id.to_owned()))
    }

    /// Where an entry's file is on this machine: always inside the vault.
    /// The index is a plain file on the user's storage (and comes back with
    /// a restore), so a `file` that tries to leave the vault (`..`, a drive,
    /// a backslash) loses those parts. Run, Show and Remove then find no
    /// file, or one whose hash doesn't match, instead of one elsewhere.
    pub fn path_of(&self, e: &Entry) -> PathBuf {
        e.file
            .split('/')
            .filter(|c| !c.is_empty() && *c != "." && *c != ".." && !c.contains(['\\', ':']))
            .fold(self.root.clone(), |p, c| p.join(c))
    }

    /// Copy `src` into the vault. The original is left where it is. The copy
    /// is hashed as it's written, under a temporary name, and only renamed
    /// into place (and indexed) once complete.
    pub fn add(
        &self,
        src: &Path,
        meta: NewEntry,
        signer: Option<String>,
        now: i64,
    ) -> Result<Added, VaultError> {
        fs::create_dir_all(&self.root).map_err(io_err(&self.root))?;
        let source_name = src
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "installer".to_owned());
        let temp = self.root.join(format!(
            "incoming-{}-{}.partial",
            std::process::id(),
            next_temp()
        ));
        let result = (|| {
            let (hash, size) = copy_hashing(src, &temp)?;
            let index = self.index()?;
            if let Some(e) = index.entries.iter().find(|e| e.hash == hash) {
                return Ok(Added::AlreadyThere(e.clone()));
            }
            let mut rel = vec![
                folder_name(&meta.vendor, "Unknown vendor"),
                folder_name(&meta.product, "Unknown product"),
                folder_name(meta.version.as_deref().unwrap_or(""), "Unknown version"),
                platform_folder(meta.os, meta.arch),
            ];
            // A different file of the same name is already there: keep both,
            // the new one in a folder named by its hash, never as "name (1)".
            let name = folder_name(&source_name, "installer");
            let mut dest = rel.iter().fold(self.root.clone(), |p, c| p.join(c));
            if dest.join(&name).exists() {
                rel.push(hash[..8].to_owned());
                dest = dest.join(&hash[..8]);
            }
            fs::create_dir_all(&dest).map_err(io_err(&dest))?;
            let dest_file = dest.join(&name);
            fs::rename(&temp, &dest_file).map_err(io_err(&dest_file))?;
            rel.push(name);
            let entry = Entry {
                id: hash[..16].to_owned(),
                hash,
                size,
                file: rel.join("/"),
                vendor: meta.vendor.trim().to_owned(),
                product: meta.product.trim().to_owned(),
                product_id: meta.product_id,
                version: meta.version.filter(|v| !v.trim().is_empty()),
                os: meta.os,
                arch: meta.arch,
                source_name,
                signer,
                added_at: now,
            };
            self.update_index(|i| {
                if !i.entries.iter().any(|e| e.hash == entry.hash) {
                    i.entries.push(entry.clone());
                }
            })?;
            Ok(Added::New(entry))
        })();
        let _ = fs::remove_file(&temp);
        result
    }

    /// Re-read the index, change it, write it back (temporary file, then
    /// rename). Reading first keeps entries another machine added meanwhile.
    /// Updates in this process take turns, so two adds at once can't lose
    /// each other's entry.
    fn update_index(&self, change: impl FnOnce(&mut Index)) -> Result<(), VaultError> {
        static INDEX_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _turn = INDEX_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut index = self.index()?;
        change(&mut index);
        index.entries.sort_by(|a, b| {
            (&a.vendor, &a.product, &a.file).cmp(&(&b.vendor, &b.product, &b.file))
        });
        let path = self.root.join(INDEX_FILE);
        let temp = self.root.join(format!(
            "{INDEX_FILE}.{}-{}.partial",
            std::process::id(),
            next_temp()
        ));
        let json = serde_json::to_string_pretty(&index).expect("index serializes");
        let write = || -> io::Result<()> {
            let mut f = File::create(&temp)?;
            f.write_all(json.as_bytes())?;
            f.sync_all()?;
            fs::rename(&temp, &path)
        };
        write().map_err(io_err(&path))
    }

    /// Take an installer out of the vault: delete Patch Pilot's copy and its
    /// index entry, and any folders left empty by it. The original download
    /// is untouched. Used to undo an add.
    pub fn remove(&self, id: &str) -> Result<Entry, VaultError> {
        let entry = self.entry(id)?;
        let path = self.path_of(&entry);
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(io_err(&path)(e)),
        }
        // Up to (not including) the root; remove_dir only removes empty ones.
        let mut dir = path.parent();
        while let Some(d) = dir.filter(|d| *d != self.root && d.starts_with(&self.root)) {
            if fs::remove_dir(d).is_err() {
                break;
            }
            dir = d.parent();
        }
        self.update_index(|i| i.entries.retain(|e| e.id != entry.id))?;
        Ok(entry)
    }

    /// True when the file is still exactly what was added.
    pub fn verify(&self, e: &Entry) -> Result<bool, VaultError> {
        let path = self.path_of(e);
        let hash = hash_file(&path).map_err(io_err(&path))?;
        Ok(hash == e.hash)
    }

    /// For each product, the best installer for `os`: one made for that OS
    /// before one that doesn't say, then the newest version.
    pub fn best_for(&self, product: &ProductId, os: Os) -> Result<Option<Entry>, VaultError> {
        Ok(best_for(&self.index()?, product, os).cloned())
    }
}

/// See [`Vault::best_for`].
pub fn best_for<'a>(index: &'a Index, product: &ProductId, os: Os) -> Option<&'a Entry> {
    index
        .entries
        .iter()
        .filter(|e| e.product_id.as_ref() == Some(product) && e.fits(os))
        .max_by(|a, b| {
            let exact = |e: &Entry| e.os == Some(os);
            exact(a).cmp(&exact(b)).then_with(|| {
                match compare_versions(
                    a.version.as_deref().unwrap_or(""),
                    b.version.as_deref().unwrap_or(""),
                ) {
                    VersionOrder::Newer => std::cmp::Ordering::Greater,
                    VersionOrder::Older => std::cmp::Ordering::Less,
                    // Unparseable: the one added later.
                    _ => a.added_at.cmp(&b.added_at),
                }
            })
        })
}

/// Installer files directly inside `dir` (not in subfolders), newest first.
/// Browsers' in-progress downloads (`.crdownload`, `.part`, `.download`)
/// don't count until they're renamed.
pub fn installers_in(dir: &Path) -> Vec<PathBuf> {
    let Ok(read) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<(std::time::SystemTime, PathBuf)> = read
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.path())
        .filter(|p| is_installer(p))
        .map(|p| {
            let mtime = fs::metadata(&p)
                .and_then(|m| m.modified())
                .unwrap_or(std::time::UNIX_EPOCH);
            (mtime, p)
        })
        .collect();
    found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    found.into_iter().map(|(_, p)| p).collect()
}

/// A number for temporary file names, unique within this process, so two
/// adds running at once never share one.
fn next_temp() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// BLAKE3 of a file, hex.
pub fn hash_file(path: &Path) -> io::Result<String> {
    let mut hasher = blake3::Hasher::new();
    hasher.update_reader(File::open(path)?)?;
    Ok(hasher.finalize().to_hex().to_string())
}

fn copy_hashing(src: &Path, dest: &Path) -> Result<(String, u64), VaultError> {
    let mut input = File::open(src).map_err(io_err(src))?;
    let mut output = File::create(dest).map_err(io_err(dest))?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0; 1 << 20];
    let mut size = 0u64;
    loop {
        let n = input.read(&mut buf).map_err(io_err(src))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        output.write_all(&buf[..n]).map_err(io_err(dest))?;
        size += n as u64;
    }
    output.sync_all().map_err(io_err(dest))?;
    Ok((hasher.finalize().to_hex().to_string(), size))
}

/// A folder or file name safe on Windows and macOS: no `\ / : * ? " < > |`
/// or control characters, no trailing dots or spaces, not a reserved name.
fn folder_name(s: &str, fallback: &str) -> String {
    let cleaned: String = s
        .trim()
        .chars()
        .map(|c| {
            if c.is_control() || r#"\/:*?"<>|"#.contains(c) {
                '-'
            } else {
                c
            }
        })
        .collect();
    let cleaned = cleaned.trim_end_matches(['.', ' ']).trim_start_matches('.');
    let reserved = {
        let base = cleaned.split('.').next().unwrap_or("").to_ascii_uppercase();
        matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || (base.len() == 4
                && (base.starts_with("COM") || base.starts_with("LPT"))
                && base.as_bytes()[3].is_ascii_digit())
    };
    if cleaned.is_empty() || reserved {
        fallback.to_owned()
    } else {
        cleaned.to_owned()
    }
}

/// `windows-x64`, `macos-universal`, `any`.
fn platform_folder(os: Option<Os>, arch: Option<Arch>) -> String {
    let os = match os {
        Some(Os::Windows) => "windows",
        Some(Os::Macos) => "macos",
        None => "any",
    };
    match arch {
        Some(Arch::X86_64) => format!("{os}-x64"),
        Some(Arch::Aarch64) => format!("{os}-arm64"),
        Some(Arch::Universal) => format!("{os}-universal"),
        None => os.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_safe_on_both_oses() {
        assert_eq!(folder_name("Pro-Q 3: Ultimate?", "x"), "Pro-Q 3- Ultimate-");
        assert_eq!(folder_name(" CON ", "x"), "x");
        assert_eq!(folder_name("v1.0.", "x"), "v1.0");
        assert_eq!(folder_name("..", "x"), "x");
        assert_eq!(folder_name("", "Unknown"), "Unknown");
    }

    #[test]
    fn platform_folders() {
        assert_eq!(
            platform_folder(Some(Os::Windows), Some(Arch::X86_64)),
            "windows-x64"
        );
        assert_eq!(platform_folder(Some(Os::Macos), None), "macos");
        assert_eq!(platform_folder(None, None), "any");
    }
}
