//! Where plug-ins live on each OS.

use std::path::PathBuf;

use patchpilot_core::{Os, PathMap, PluginFormat};

/// A folder to search for plug-ins of one format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginRoot {
    /// Logical root name used in manifests, e.g. `VST3_SYSTEM`.
    pub name: String,
    /// Format a bare `.dll` in this folder is treated as. Bundles and files
    /// with a format extension are classified by extension wherever they are.
    pub format: PluginFormat,
    pub path: PathBuf,
}

impl PluginRoot {
    pub fn new(name: impl Into<String>, format: PluginFormat, path: impl Into<PathBuf>) -> Self {
        Self {
            name: name.into(),
            format,
            path: path.into(),
        }
    }
}

/// The per-OS folders the default roots are built from.
///
/// Passed in rather than read inside [`default_roots`] so both OSes can be
/// tested anywhere.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemDirs {
    pub os: Os,
    /// Windows `%ProgramFiles%`. Unused on macOS.
    pub program_files: String,
    /// Windows `%CommonProgramFiles%`. Unused on macOS.
    pub common_program_files: String,
    /// Windows `%LOCALAPPDATA%`. Unused on macOS.
    pub local_app_data: String,
    /// macOS home folder. Unused on Windows.
    pub home: String,
}

impl SystemDirs {
    /// Read the folders for the OS this build runs on.
    pub fn from_env() -> Option<Self> {
        let os = Os::current()?;
        let var = |k: &str, default: &str| std::env::var(k).unwrap_or_else(|_| default.to_owned());
        Some(Self {
            os,
            program_files: var("ProgramFiles", r"C:\Program Files"),
            common_program_files: var("CommonProgramFiles", r"C:\Program Files\Common Files"),
            local_app_data: var("LOCALAPPDATA", ""),
            home: var("HOME", ""),
        })
    }
}

/// The standard plug-in folders for an OS. Folders that don't exist are
/// skipped by the walk, so every candidate is listed.
pub fn default_roots(dirs: &SystemDirs) -> Vec<PluginRoot> {
    use PluginFormat::*;
    let mut roots = Vec::new();
    let mut add = |name: &str, format, path: String| {
        // An unset base (e.g. no HOME) would make the path relative; skip it.
        if !path.starts_with(['\\', '/']) && !path.get(1..2).is_some_and(|c| c == ":") {
            return;
        }
        roots.push(PluginRoot::new(name, format, path));
    };
    match dirs.os {
        Os::Windows => {
            let common = dirs.common_program_files.trim_end_matches('\\');
            let pf = dirs.program_files.trim_end_matches('\\');
            let local = dirs.local_app_data.trim_end_matches('\\');
            add("VST3_SYSTEM", Vst3, format!(r"{common}\VST3"));
            add("CLAP_SYSTEM", Clap, format!(r"{common}\CLAP"));
            add("AAX_SYSTEM", Aax, format!(r"{common}\Avid\Audio\Plug-Ins"));
            add("VST2_COMMON", Vst2, format!(r"{common}\VST2"));
            add(
                "VST2_STEINBERG_COMMON",
                Vst2,
                format!(r"{common}\Steinberg\VST2"),
            );
            add("VST2_PROGRAM_FILES", Vst2, format!(r"{pf}\VSTPlugins"));
            add(
                "VST2_STEINBERG",
                Vst2,
                format!(r"{pf}\Steinberg\VSTPlugins"),
            );
            if !local.is_empty() {
                add("VST3_USER", Vst3, format!(r"{local}\Programs\Common\VST3"));
                add("CLAP_USER", Clap, format!(r"{local}\Programs\Common\CLAP"));
            }
        }
        Os::Macos => {
            let home = dirs.home.trim_end_matches('/');
            for (prefix, base) in [("SYSTEM", String::new()), ("USER", home.to_owned())] {
                if prefix == "USER" && home.is_empty() {
                    continue;
                }
                let lib = format!("{base}/Library/Audio/Plug-Ins");
                add(&format!("AU_{prefix}"), Au, format!("{lib}/Components"));
                add(&format!("VST2_{prefix}"), Vst2, format!("{lib}/VST"));
                add(&format!("VST3_{prefix}"), Vst3, format!("{lib}/VST3"));
                add(&format!("CLAP_{prefix}"), Clap, format!("{lib}/CLAP"));
            }
            add(
                "AAX_SYSTEM",
                Aax,
                "/Library/Application Support/Avid/Audio/Plug-Ins".to_owned(),
            );
        }
    }
    roots
}

/// A path map covering `roots`, for turning scan results into portable paths.
pub fn path_map(os: Os, roots: &[PluginRoot]) -> PathMap {
    roots.iter().fold(PathMap::new(os), |map, r| {
        map.with_root(r.name.clone(), r.path.to_string_lossy().into_owned())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(roots: &[PluginRoot]) -> Vec<(String, String)> {
        roots
            .iter()
            .map(|r| (r.name.clone(), r.path.to_string_lossy().into_owned()))
            .collect()
    }

    #[test]
    fn windows_defaults() {
        let dirs = SystemDirs {
            os: Os::Windows,
            program_files: r"C:\Program Files".into(),
            common_program_files: r"C:\Program Files\Common Files".into(),
            local_app_data: r"C:\Users\alice\AppData\Local".into(),
            home: String::new(),
        };
        let roots = paths(&default_roots(&dirs));
        for (name, path) in [
            ("VST3_SYSTEM", r"C:\Program Files\Common Files\VST3"),
            ("CLAP_SYSTEM", r"C:\Program Files\Common Files\CLAP"),
            (
                "AAX_SYSTEM",
                r"C:\Program Files\Common Files\Avid\Audio\Plug-Ins",
            ),
            ("VST2_PROGRAM_FILES", r"C:\Program Files\VSTPlugins"),
            (
                "VST3_USER",
                r"C:\Users\alice\AppData\Local\Programs\Common\VST3",
            ),
        ] {
            assert!(
                roots.contains(&(name.into(), path.into())),
                "{name}: {roots:?}"
            );
        }
    }

    #[test]
    fn macos_defaults() {
        let dirs = SystemDirs {
            os: Os::Macos,
            program_files: String::new(),
            common_program_files: String::new(),
            local_app_data: String::new(),
            home: "/Users/alice".into(),
        };
        let roots = paths(&default_roots(&dirs));
        for (name, path) in [
            ("AU_SYSTEM", "/Library/Audio/Plug-Ins/Components"),
            ("VST3_SYSTEM", "/Library/Audio/Plug-Ins/VST3"),
            ("CLAP_USER", "/Users/alice/Library/Audio/Plug-Ins/CLAP"),
            ("VST2_USER", "/Users/alice/Library/Audio/Plug-Ins/VST"),
            (
                "AAX_SYSTEM",
                "/Library/Application Support/Avid/Audio/Plug-Ins",
            ),
        ] {
            assert!(
                roots.contains(&(name.into(), path.into())),
                "{name}: {roots:?}"
            );
        }
    }

    #[test]
    fn missing_home_skips_user_roots() {
        let dirs = SystemDirs {
            os: Os::Macos,
            program_files: String::new(),
            common_program_files: String::new(),
            local_app_data: String::new(),
            home: String::new(),
        };
        assert!(
            default_roots(&dirs)
                .iter()
                .all(|r| !r.name.ends_with("_USER"))
        );
    }

    #[test]
    fn same_logical_names_on_both_oses() {
        let win = SystemDirs {
            os: Os::Windows,
            program_files: r"C:\Program Files".into(),
            common_program_files: r"C:\Program Files\Common Files".into(),
            local_app_data: String::new(),
            home: String::new(),
        };
        let mac = SystemDirs {
            os: Os::Macos,
            home: String::new(),
            ..win.clone()
        };
        let names = |d| -> Vec<String> { default_roots(d).into_iter().map(|r| r.name).collect() };
        let (win, mac) = (names(&win), names(&mac));
        for shared in ["VST3_SYSTEM", "CLAP_SYSTEM", "AAX_SYSTEM"] {
            assert!(win.iter().any(|n| n == shared) && mac.iter().any(|n| n == shared));
        }
    }
}
