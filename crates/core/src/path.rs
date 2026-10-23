use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::Os;

/// A path as stored in a manifest.
///
/// Prefer [`PortablePath::Logical`]: a named root such as `SAMPLES_ROOT` plus a
/// `/`-separated relative path, resolved per machine through a [`PathMap`].
/// [`PortablePath::Absolute`] is the fallback for paths outside every root, and
/// is only meaningful on the machine that wrote it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PortablePath {
    Logical {
        /// Logical root name, e.g. `VST3_SYSTEM` or `SAMPLES_ROOT`.
        root: String,
        /// Path below the root, `/`-separated on every OS. Empty for the root itself.
        rel: String,
    },
    /// A machine-specific absolute path, as the OS spells it.
    Absolute(String),
}

impl PortablePath {
    pub fn logical(root: impl Into<String>, rel: impl Into<String>) -> Self {
        Self::Logical {
            root: root.into(),
            rel: rel.into(),
        }
    }
}

/// Maps logical roots to physical folders on one machine.
///
/// Works on path strings rather than `std::path::Path` so a Windows map can be
/// built and tested on macOS and vice versa.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PathMap {
    pub os: Os,
    /// Logical root name to physical folder, as the OS spells it.
    pub roots: BTreeMap<String, String>,
}

impl PathMap {
    pub fn new(os: Os) -> Self {
        Self {
            os,
            roots: BTreeMap::new(),
        }
    }

    pub fn with_root(mut self, name: impl Into<String>, physical: impl Into<String>) -> Self {
        self.roots.insert(name.into(), physical.into());
        self
    }

    /// Express a physical path relative to the most specific root containing it.
    ///
    /// Matching is ASCII case-insensitive on both OSes, since NTFS and the
    /// default APFS volume are both case-insensitive.
    pub fn to_portable(&self, physical: &str) -> PortablePath {
        let path = self.normalize(physical);
        let best = self
            .roots
            .iter()
            .filter_map(|(name, root)| {
                let root = self.normalize(root);
                strip_root(&path, &root).map(|rel| (name, root.len(), rel))
            })
            .max_by_key(|&(_, len, _)| len);
        match best {
            Some((name, _, rel)) => PortablePath::logical(name.clone(), rel),
            None => PortablePath::Absolute(physical.to_owned()),
        }
    }

    /// Turn a portable path back into a physical path on this machine.
    ///
    /// Returns `None` when the root is not mapped here, or when `rel` tries to
    /// escape its root (`..`, absolute or drive-prefixed components), since
    /// manifests can come from other machines.
    pub fn resolve(&self, path: &PortablePath) -> Option<String> {
        match path {
            PortablePath::Absolute(p) => Some(p.clone()),
            PortablePath::Logical { root, rel } => {
                let base = self.roots.get(root)?;
                if !is_safe_rel(rel) {
                    return None;
                }
                let sep = self.os.separator();
                let mut out = base.trim_end_matches(['/', '\\']).to_owned();
                for part in rel.split('/').filter(|p| !p.is_empty()) {
                    out.push(sep);
                    out.push_str(part);
                }
                Some(out)
            }
        }
    }

    fn normalize(&self, p: &str) -> String {
        let p = match self.os {
            Os::Windows => p.replace('\\', "/"),
            Os::Macos => p.to_owned(),
        };
        p.trim_end_matches('/').to_owned()
    }
}

/// `rel` of `path` below `root`, matching whole components only.
fn strip_root(path: &str, root: &str) -> Option<String> {
    let head = path.get(..root.len())?;
    if !head.eq_ignore_ascii_case(root) {
        return None;
    }
    match &path[root.len()..] {
        "" => Some(String::new()),
        rest => rest.strip_prefix('/').map(str::to_owned),
    }
}

fn is_safe_rel(rel: &str) -> bool {
    !rel.starts_with('/')
        && rel
            .split('/')
            .all(|part| part != ".." && !part.contains(['\\', ':']))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win() -> PathMap {
        PathMap::new(Os::Windows)
            .with_root("COMMON_FILES", r"C:\Program Files\Common Files")
            .with_root("VST3_SYSTEM", r"C:\Program Files\Common Files\VST3")
            .with_root("SAMPLES_ROOT", r"D:\Samples\")
    }

    fn mac() -> PathMap {
        PathMap::new(Os::Macos)
            .with_root("VST3_SYSTEM", "/Library/Audio/Plug-Ins/VST3")
            .with_root("SAMPLES_ROOT", "/Volumes/Samples")
    }

    #[test]
    fn most_specific_root_wins() {
        assert_eq!(
            win().to_portable(r"C:\Program Files\Common Files\VST3\Acme\Verb.vst3"),
            PortablePath::logical("VST3_SYSTEM", "Acme/Verb.vst3")
        );
    }

    #[test]
    fn windows_matching_ignores_case_and_trailing_separators() {
        assert_eq!(
            win().to_portable(r"d:\samples\Drums\kick.wav"),
            PortablePath::logical("SAMPLES_ROOT", "Drums/kick.wav")
        );
        assert_eq!(
            win().to_portable(r"D:\Samples"),
            PortablePath::logical("SAMPLES_ROOT", "")
        );
    }

    #[test]
    fn only_whole_components_match() {
        assert_eq!(
            mac().to_portable("/Volumes/SamplesOld/a.wav"),
            PortablePath::Absolute("/Volumes/SamplesOld/a.wav".into())
        );
    }

    #[test]
    fn same_logical_path_resolves_per_os() {
        let p = PortablePath::logical("VST3_SYSTEM", "Acme/Verb.vst3");
        assert_eq!(
            win().resolve(&p).unwrap(),
            r"C:\Program Files\Common Files\VST3\Acme\Verb.vst3"
        );
        assert_eq!(
            mac().resolve(&p).unwrap(),
            "/Library/Audio/Plug-Ins/VST3/Acme/Verb.vst3"
        );
    }

    #[test]
    fn round_trips_through_portable() {
        let m = win();
        let physical = r"D:\Samples\Drums\kick.wav";
        assert_eq!(m.resolve(&m.to_portable(physical)).unwrap(), physical);
    }

    #[test]
    fn resolve_rejects_unmapped_roots_and_escapes() {
        let m = mac();
        assert_eq!(m.resolve(&PortablePath::logical("CLAP_SYSTEM", "x")), None);
        for rel in ["../etc/passwd", "a/../../b", "/etc", r"a\..\b", "C:/x"] {
            assert_eq!(
                m.resolve(&PortablePath::logical("SAMPLES_ROOT", rel)),
                None,
                "{rel}"
            );
        }
    }
}
