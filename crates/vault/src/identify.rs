//! Telling what an installer is from its file name.
//!
//! Installers carry no metadata we can read without running them, so the
//! file name is the best clue: `FabFilter Pro-Q 3 v3.21 Installer.exe`,
//! `Diva_145_12345_Mac.zip`, `ValhallaRoom_2_0_Win.zip`. The guess is a
//! starting point the user can correct before anything is added.

use std::path::Path;

use patchpilot_core::{Arch, Os, Product};
use serde::Serialize;

/// File types the vault accepts.
pub const INSTALLER_EXTENSIONS: [&str; 6] = ["exe", "msi", "pkg", "mpkg", "dmg", "zip"];

/// True for a file whose extension is an installer type.
pub fn is_installer(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| INSTALLER_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

/// What the file name says.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Guess {
    /// The file name without version, OS, architecture and words like
    /// "Installer", e.g. `FabFilter Pro-Q 3`.
    pub name: String,
    pub version: Option<String>,
    /// From the extension (`.exe`, `.pkg`) or a word in the name; `None` for
    /// a `.zip` that doesn't say.
    pub os: Option<Os>,
    pub arch: Option<Arch>,
}

/// Read version, OS and architecture out of an installer's file name.
pub fn guess(file_name: &str) -> Guess {
    let (stem, ext) = match file_name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s, e.to_ascii_lowercase()),
        _ => (file_name, String::new()),
    };
    let mut out = Guess {
        os: match ext.as_str() {
            "exe" | "msi" => Some(Os::Windows),
            "pkg" | "mpkg" | "dmg" => Some(Os::Macos),
            _ => None,
        },
        ..Guess::default()
    };

    let mut words = Vec::new();
    for token in stem.split(|c: char| c.is_whitespace() || c == '_' || c == '+') {
        // "Pro-Q" keeps its hyphen; "Serum-x64-Setup" doesn't need to.
        let parts: Vec<&str> = if token.contains('-') && token.split('-').any(is_noise) {
            token.split('-').collect()
        } else {
            vec![token]
        };
        for part in parts {
            let lower = part.to_ascii_lowercase();
            if let Some(os) = os_word(&lower) {
                out.os.get_or_insert(os);
            } else if let Some(arch) = arch_word(&lower) {
                out.arch.get_or_insert(arch);
            } else if is_noise(part) {
            } else if !part.is_empty() {
                words.push(part);
            }
        }
    }

    // The version starts at the last word that clearly is one ("v3.21",
    // "1.368"); failing that, it's a trailing run of two or more numbers
    // ("2 0 5" from ValhallaRoom_2_0_5, "145 12345" from Diva_145_12345). A
    // lone trailing number stays in the name: "Pro-Q 3".
    let is_num = |w: &str| !w.is_empty() && w.chars().all(|c| c.is_ascii_digit());
    let span = match (1..words.len()).rev().find(|&i| is_version_start(words[i])) {
        Some(start) => {
            let mut end = start + 1;
            while end < words.len() && is_num(words[end]) {
                end += 1;
            }
            Some((start, end))
        }
        None => {
            let end = words.len();
            let mut start = end;
            while start > 1 && is_num(words[start - 1]) {
                start -= 1;
            }
            (end - start >= 2).then_some((start, end))
        }
    };
    if let Some((start, end)) = span {
        out.version = Some(version_string(&words[start..end]));
        words.drain(start..end);
    }
    out.name = words.join(" ").trim_matches(['-', '.', ' ']).to_owned();
    out
}

fn os_word(w: &str) -> Option<Os> {
    match w {
        "win" | "win32" | "win64" | "windows" | "pc" => Some(Os::Windows),
        "mac" | "macos" | "osx" | "macosx" | "apple" => Some(Os::Macos),
        _ => None,
    }
}

fn arch_word(w: &str) -> Option<Arch> {
    match w {
        "x64" | "x86_64" | "amd64" | "64bit" | "64-bit" | "intel" => Some(Arch::X86_64),
        "arm64" | "aarch64" | "arm" => Some(Arch::Aarch64),
        "universal" | "ub2" | "ub" => Some(Arch::Universal),
        _ => None,
    }
}

fn is_noise(w: &str) -> bool {
    matches!(
        w.to_ascii_lowercase().as_str(),
        "installer"
            | "install"
            | "setup"
            | "full"
            | "x86"
            | "32bit"
            | "vst"
            | "vst2"
            | "vst3"
            | "au"
            | "aax"
            | "clap"
            | "plugin"
            | "plugins"
            | "plug-in"
            | "plug-ins"
            | "win"
            | "mac"
            | "x64"
            | "release"
            | "-"
    ) || os_word(&w.to_ascii_lowercase()).is_some()
        || arch_word(&w.to_ascii_lowercase()).is_some()
}

/// A word that clearly is a version: `v3`, `V4`, `3.21`, `1.4b3`.
fn is_version_start(w: &str) -> bool {
    let digits = w.strip_prefix(['v', 'V']).unwrap_or(w);
    digits.starts_with(|c: char| c.is_ascii_digit())
        && (digits.len() < w.len() || digits.contains('.'))
        && digits
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.')
}

/// `v3.21` → `3.21`; `1 4 5` → `1.4.5`; `145 12345` → `145` (a long number
/// after the version is a build or order number).
fn version_string(words: &[&str]) -> String {
    let first = words[0].strip_prefix(['v', 'V']).unwrap_or(words[0]);
    if first.contains('.') {
        return first.to_owned();
    }
    let mut nums = vec![first];
    for w in &words[1..] {
        if w.len() >= 4 {
            break;
        }
        nums.push(w);
    }
    nums.join(".")
}

/// Squash a name to lowercase letters and digits for loose matching.
pub fn compact(s: &str) -> String {
    s.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// The known product this installer most likely installs: the one whose name
/// appears in the file name (letters and digits only, ignoring case), longest
/// name first, with the vendor's name as a tie-breaker. Names under three
/// characters are ignored; they match too much.
pub fn match_product<'a>(file_name: &str, products: &'a [Product]) -> Option<&'a Product> {
    let stem = compact(file_name.rsplit_once('.').map_or(file_name, |(s, _)| s));
    products
        .iter()
        .filter_map(|p| {
            let name = compact(&p.name);
            if name.len() < 3 || !stem.contains(&name) {
                return None;
            }
            let vendor = compact(&p.vendor);
            let vendor_hit = !vendor.is_empty() && stem.contains(&vendor);
            Some(((name.len(), vendor_hit), p))
        })
        .max_by_key(|(rank, _)| *rank)
        .map(|(_, p)| p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use patchpilot_core::ProductId;

    fn g(name: &str) -> (String, Option<String>, Option<Os>, Option<Arch>) {
        let g = guess(name);
        (g.name, g.version, g.os, g.arch)
    }

    #[test]
    fn reads_common_installer_names() {
        assert_eq!(
            g("FabFilter Pro-Q 3 v3.21 Installer.exe"),
            (
                "FabFilter Pro-Q 3".into(),
                Some("3.21".into()),
                Some(Os::Windows),
                None
            )
        );
        assert_eq!(
            g("Diva_145_12345_Mac.zip"),
            ("Diva".into(), Some("145".into()), Some(Os::Macos), None)
        );
        assert_eq!(
            g("ValhallaRoom_2_0_5_Win.zip"),
            (
                "ValhallaRoom".into(),
                Some("2.0.5".into()),
                Some(Os::Windows),
                None
            )
        );
        assert_eq!(
            g("Serum_x64_Setup_1.368.exe"),
            (
                "Serum".into(),
                Some("1.368".into()),
                Some(Os::Windows),
                Some(Arch::X86_64)
            )
        );
        assert_eq!(
            g("Kilohearts Installer.pkg"),
            ("Kilohearts".into(), None, Some(Os::Macos), None)
        );
        assert_eq!(
            g("Soothe2-macOS-universal.dmg"),
            (
                "Soothe2".into(),
                None,
                Some(Os::Macos),
                Some(Arch::Universal)
            )
        );
    }

    #[test]
    fn only_installer_types_count() {
        assert!(is_installer(Path::new("a/Setup.EXE")));
        assert!(is_installer(Path::new("Thing.dmg")));
        assert!(!is_installer(Path::new("Thing.dmg.crdownload")));
        assert!(!is_installer(Path::new("song.wav")));
    }

    fn product(vendor: &str, name: &str) -> Product {
        Product {
            canonical_id: ProductId(format!("{}/{}", compact(vendor), compact(name))),
            vendor: vendor.into(),
            name: name.into(),
        }
    }

    #[test]
    fn matches_the_longest_product_name_in_the_file_name() {
        let products = [
            product("FabFilter", "Pro-Q"),
            product("FabFilter", "Pro-Q 3"),
            product("FabFilter", "Pro-L 2"),
            product("u-he", "Diva"),
            product("Acme", "EQ"),
        ];
        let hit = |f| match_product(f, &products).map(|p| p.name.as_str());
        assert_eq!(
            hit("FabFilter Pro-Q 3 v3.21 Installer.exe"),
            Some("Pro-Q 3")
        );
        assert_eq!(hit("Diva_145_12345_Mac.zip"), Some("Diva"));
        assert_eq!(hit("Equalizer.pkg"), None, "two-letter names don't match");
        assert_eq!(hit("Something Else.exe"), None);
    }
}
