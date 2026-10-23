//! CLAP identity from Reaper's plug-in cache (#8, decision 21).
//!
//! CLAP has no static descriptor, so the real plug-in IDs only exist inside
//! the binary. Reaper loads each `.clap` once when it scans and writes what it
//! found to `reaper-clap-<platform>.ini` in its resource folder. Reading that
//! file gives the IDs without Patch Pilot running any plug-in code.
//!
//! The format is undocumented by Cockos. Confirmed against a real Windows
//! `reaper-clap-win64.ini` (#32, `fixtures/scanner/clap/reaper-real/`):
//!
//! ```ini
//! [FabFilter Pro-Q 3.clap]
//! _=001634CF0C58DA010696D9750F99DA01
//! com.FabFilter.preset-discovery.Pro-Q.3=0|Pro-Q 3 (FabFilter)
//! ```
//!
//! One section per file name (CRLF line endings), a `_` line, then
//! `<clap-id>=<flags>|<name> (<vendor>)`, where flags is `0` for an effect
//! and `1` for an instrument. The `_` value is two little-endian Windows
//! FILETIMEs: the plug-in file's mtime, then when Reaper scanned it. We
//! ignore it for now. Anything else is skipped. An entry is trusted only
//! when the cache file is newer than the plug-in, since Reaper may not have
//! rescanned an updated plug-in. The macOS file is not yet confirmed.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// One plug-in inside a `.clap` file, as Reaper saw it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClapCacheEntry {
    /// The CLAP plug-in ID, e.g. `com.acmeaudio.synth`.
    pub id: String,
    pub name: String,
    pub vendor: Option<String>,
}

/// Parsed `reaper-clap-*.ini` files, keyed by lowercase file name.
#[derive(Debug, Clone, Default)]
pub struct ClapCache {
    files: HashMap<String, CachedFile>,
}

#[derive(Debug, Clone)]
struct CachedFile {
    entries: Vec<ClapCacheEntry>,
    /// When the cache file that held this section was last written.
    written: Option<SystemTime>,
}

impl ClapCache {
    /// Parse one cache file's text. `written` is the cache file's mtime.
    pub fn parse(text: &str, written: Option<SystemTime>) -> Self {
        let mut cache = Self::default();
        cache.add(text, written);
        cache
    }

    /// Load every `reaper-clap-*.ini` in a Reaper resource folder. Missing
    /// folder or unreadable files give an empty cache. When two files list
    /// the same plug-in file (e.g. arm64 and x86_64 caches), the first in
    /// name order wins.
    pub fn load_reaper(resource_dir: &Path) -> Self {
        let mut cache = Self::default();
        let Ok(dir) = std::fs::read_dir(resource_dir) else {
            return cache;
        };
        let mut paths: Vec<PathBuf> = dir
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| {
                p.file_name().is_some_and(|n| {
                    let n = n.to_string_lossy().to_lowercase();
                    n.starts_with("reaper-clap-") && n.ends_with(".ini")
                })
            })
            .collect();
        paths.sort();
        for path in paths {
            if let Ok(bytes) = std::fs::read(&path) {
                let written = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
                cache.add(&String::from_utf8_lossy(&bytes), written);
            }
        }
        cache
    }

    fn add(&mut self, text: &str, written: Option<SystemTime>) {
        let mut section: Option<String> = None;
        let mut entries = Vec::new();
        let mut flush = |section: &mut Option<String>, entries: &mut Vec<ClapCacheEntry>| {
            if let Some(name) = section.take() {
                let entries = std::mem::take(entries);
                if !entries.is_empty() {
                    self.files
                        .entry(name)
                        .or_insert(CachedFile { entries, written });
                }
            }
        };
        for line in text.lines() {
            let line = line.trim();
            if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                flush(&mut section, &mut entries);
                section = Some(name.trim().to_lowercase());
                continue;
            }
            if section.is_none() {
                continue;
            }
            if let Some(entry) = parse_entry(line) {
                entries.push(entry);
            }
        }
        flush(&mut section, &mut entries);
    }

    /// Plug-ins Reaper found in `plugin` (matched by file name), or `None`
    /// when the file isn't cached or changed after Reaper last scanned it.
    pub fn lookup(&self, plugin: &Path) -> Option<&[ClapCacheEntry]> {
        let name = plugin.file_name()?.to_string_lossy().to_lowercase();
        let cached = self.files.get(&name)?;
        let modified = std::fs::metadata(plugin).and_then(|m| m.modified()).ok();
        match (cached.written, modified) {
            (Some(written), Some(modified)) if modified > written => None,
            _ => Some(&cached.entries),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

fn parse_entry(line: &str) -> Option<ClapCacheEntry> {
    let (id, value) = line.split_once('=')?;
    let id = id.trim();
    if id.is_empty() || id == "_" || id.starts_with(';') || id.starts_with('#') {
        return None;
    }
    // Optional numeric flags before `|`.
    let value = match value.split_once('|') {
        Some((flags, rest)) if flags.trim().chars().all(|c| c.is_ascii_digit()) => rest,
        _ => value,
    };
    let (name, vendor) = split_vendor(value.trim());
    if name.is_empty() {
        return None;
    }
    Some(ClapCacheEntry {
        id: id.to_owned(),
        name: name.to_owned(),
        vendor: vendor.map(str::to_owned),
    })
}

/// `"Acme Synth (Acme Audio)"` → `("Acme Synth", Some("Acme Audio"))`.
/// Only the last parenthesized group is the vendor.
fn split_vendor(s: &str) -> (&str, Option<&str>) {
    let split = s.strip_suffix(')').and_then(|inner| {
        let open = inner.rfind(" (")?;
        let (name, vendor) = (inner[..open].trim(), inner[open + 2..].trim());
        (!vendor.is_empty() && !name.is_empty()).then_some((name, Some(vendor)))
    });
    split.unwrap_or((s, None))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries() {
        let e = parse_entry("com.acme.synth=1|Acme Synth (Acme Audio)").unwrap();
        assert_eq!(e.id, "com.acme.synth");
        assert_eq!(e.name, "Acme Synth");
        assert_eq!(e.vendor.as_deref(), Some("Acme Audio"));

        let e = parse_entry("org.x.eq=Pro EQ (Mono) (X Labs)").unwrap();
        assert_eq!(e.name, "Pro EQ (Mono)");
        assert_eq!(e.vendor.as_deref(), Some("X Labs"));

        let e = parse_entry("org.x.bare=0|Bare").unwrap();
        assert_eq!((e.name.as_str(), e.vendor), ("Bare", None));

        assert!(parse_entry("_=00F0A1B2").is_none());
        assert!(parse_entry("junk").is_none());
        assert!(parse_entry("org.x.empty=0|").is_none());
    }
}
