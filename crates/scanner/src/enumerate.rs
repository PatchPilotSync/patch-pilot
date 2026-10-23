//! Find plug-in bundles and files without opening them.

use std::collections::HashSet;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use patchpilot_core::PluginFormat;
use walkdir::WalkDir;

use crate::PluginRoot;

/// A plug-in found on disk, before any metadata is read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub format: PluginFormat,
    /// Path as found under the root (symlinks not resolved).
    pub path: PathBuf,
    /// Logical name of the root it was found under.
    pub root: String,
    /// True for bundle folders (`.vst3`, `.component`, ...), false for single files.
    pub is_bundle: bool,
}

/// Something that stopped part of a walk. The rest of the walk still runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalkWarning {
    pub path: Option<PathBuf>,
    pub message: String,
}

#[derive(Debug, Default)]
pub struct Enumeration {
    pub candidates: Vec<Candidate>,
    pub warnings: Vec<WalkWarning>,
}

/// Walk every root in parallel and list plug-in candidates.
///
/// Roots that don't exist are skipped silently. Symlinks are followed (plug-in
/// folders often link into other drives), loops are reported and skipped, and
/// the same target reached twice is listed once. Results are sorted by path so
/// output is stable.
pub fn enumerate(roots: &[PluginRoot]) -> Enumeration {
    let per_root: Vec<Enumeration> = std::thread::scope(|s| {
        let handles: Vec<_> = roots.iter().map(|r| s.spawn(|| walk_root(r))).collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("walker thread panicked"))
            .collect()
    });

    let mut out = Enumeration::default();
    let mut seen = HashSet::new();
    for e in per_root {
        out.warnings.extend(e.warnings);
        for c in e.candidates {
            let key = std::fs::canonicalize(&c.path).unwrap_or_else(|_| c.path.clone());
            if seen.insert(key) {
                out.candidates.push(c);
            }
        }
    }
    out.candidates.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

fn walk_root(root: &PluginRoot) -> Enumeration {
    let mut out = Enumeration::default();
    if !root.path.is_dir() {
        return out;
    }
    let mut walk = WalkDir::new(&root.path).follow_links(true).into_iter();
    while let Some(entry) = walk.next() {
        let entry = match entry {
            Ok(e) => e,
            Err(err) => {
                out.warnings.push(WalkWarning {
                    path: err.path().map(Path::to_path_buf),
                    message: err.to_string(),
                });
                continue;
            }
        };
        if entry.depth() == 0 {
            continue;
        }
        let is_dir = entry.file_type().is_dir();
        if let Some(format) = classify(entry.path(), is_dir, root.format) {
            out.candidates.push(Candidate {
                format,
                path: entry.path().to_path_buf(),
                root: root.name.clone(),
                is_bundle: is_dir,
            });
            if is_dir {
                // A bundle is one item; its insides are not separate plug-ins.
                walk.skip_current_dir();
            }
        }
    }
    out
}

/// The format of a path, or `None` if it isn't a plug-in.
fn classify(path: &Path, is_dir: bool, root_format: PluginFormat) -> Option<PluginFormat> {
    use PluginFormat::*;
    let ext = path
        .extension()
        .and_then(OsStr::to_str)?
        .to_ascii_lowercase();
    match (ext.as_str(), is_dir) {
        // Folders on macOS, and the VST3/CLAP bundle layout on Windows.
        ("vst3", _) => Some(Vst3),
        ("clap", _) => Some(Clap),
        ("component", true) => Some(Au),
        ("aaxplugin", true) => Some(Aax),
        ("vst", true) => Some(Vst2),
        // A bare DLL is only a VST2 plug-in where VST2 plug-ins are expected.
        ("dll", false) if root_format == Vst2 => Some(Vst2),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use PluginFormat::*;

    #[test]
    fn classifies_by_extension() {
        let c = |p: &str, dir| classify(Path::new(p), dir, Vst3);
        assert_eq!(c("Verb.vst3", true), Some(Vst3));
        assert_eq!(c("Verb.VST3", false), Some(Vst3));
        assert_eq!(c("Verb.component", true), Some(Au));
        assert_eq!(c("Verb.component", false), None);
        assert_eq!(c("Verb.aaxplugin", true), Some(Aax));
        assert_eq!(c("Verb.vst", true), Some(Vst2));
        assert_eq!(c("Verb.dll", false), None);
        assert_eq!(classify(Path::new("Verb.dll"), false, Vst2), Some(Vst2));
        assert_eq!(c("readme.txt", false), None);
        assert_eq!(c("Contents", true), None);
    }
}
