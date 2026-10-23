//! Compare vendor version strings.
//!
//! Vendors write versions many ways: `4.0.2`, `4.02`, `v4 build 1234`,
//! `3.0 (1234)`, `1.4b3`, `2.1.0.7`. [`compare_versions`] parses what it can
//! and says [`VersionOrder::Unknown`] when it can't, rather than guessing.

use std::cmp::Ordering;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    /// Numeric release parts, e.g. `[4, 0, 2]`.
    pub parts: Vec<u64>,
    /// Pre-release tag and number (`b3` → `("b", 3)`). Sorts before the release.
    pub pre: Option<(String, u64)>,
    /// Build number from `build 1234` or `(1234)`. Only compared when both sides have one.
    pub build: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionOrder {
    Older,
    Same,
    Newer,
    /// At least one side didn't parse and the strings differ.
    Unknown,
}

impl Version {
    /// Parse a vendor version string. `None` if there are no digits to work with.
    ///
    /// A two-digit part with a leading zero is read as two parts, so `4.02`
    /// equals `4.0.2`. `4.10` stays `[4, 10]`.
    pub fn parse(raw: &str) -> Option<Self> {
        let s = raw.trim().to_ascii_lowercase();
        let s = s
            .strip_prefix("version")
            .or_else(|| s.strip_prefix('v'))
            .unwrap_or(&s)
            .trim_start();

        // Split off a trailing build: "... build 1234", "... (1234)", "... b1234"
        // is a pre-release, not a build, so only the two explicit forms count.
        let (head, build) = split_build(s);

        let mut parts = Vec::new();
        let mut rest = head;
        loop {
            let digits = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
            if digits == 0 {
                break;
            }
            let (num, after) = rest.split_at(digits);
            if num.len() == 2 && num.starts_with('0') && !parts.is_empty() {
                parts.push(0);
                parts.push(u64::from(num.as_bytes()[1] - b'0'));
            } else {
                parts.push(num.parse().ok()?);
            }
            match after.strip_prefix('.') {
                Some(next) if next.starts_with(|c: char| c.is_ascii_digit()) => rest = next,
                _ => {
                    rest = after;
                    break;
                }
            }
        }
        if parts.is_empty() {
            return None;
        }
        let pre = parse_pre(rest.trim_start_matches(['-', ' ', '_']));
        Some(Self { parts, pre, build })
    }
}

fn split_build(s: &str) -> (&str, Option<u64>) {
    if let Some(i) = s.rfind("build") {
        let num = s[i + 5..].trim().trim_start_matches(['.', ':', '#']).trim();
        if let Ok(n) = num.parse() {
            return (s[..i].trim_end_matches([' ', '-', '_', ',']), Some(n));
        }
    }
    if let Some(inner) = s.strip_suffix(')').and_then(|t| t.rsplit_once('(')) {
        if let Ok(n) = inner.1.trim().parse() {
            return (inner.0.trim_end(), Some(n));
        }
    }
    (s, None)
}

/// `b3`, `beta2`, `rc1`, `alpha` → tag + number. Anything else is ignored.
fn parse_pre(s: &str) -> Option<(String, u64)> {
    let tag: String = s.chars().take_while(char::is_ascii_alphabetic).collect();
    let tag = match tag.as_str() {
        "a" | "alpha" => "alpha",
        "b" | "beta" => "beta",
        "rc" => "rc",
        "pre" | "preview" | "dev" => "pre",
        _ => return None,
    };
    let num = s
        .trim_start_matches(|c: char| c.is_ascii_alphabetic())
        .trim_start_matches(['.', '-'])
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .unwrap_or(0);
    Some((tag.to_owned(), num))
}

fn pre_rank(tag: &str) -> u8 {
    match tag {
        "pre" => 0,
        "alpha" => 1,
        "beta" => 2,
        _ => 3, // rc
    }
}

/// How `ours` relates to `theirs`: `Older` means `ours` is behind.
pub fn compare_versions(ours: &str, theirs: &str) -> VersionOrder {
    if ours.trim().eq_ignore_ascii_case(theirs.trim()) {
        return VersionOrder::Same;
    }
    let (Some(a), Some(b)) = (Version::parse(ours), Version::parse(theirs)) else {
        return VersionOrder::Unknown;
    };
    let len = a.parts.len().max(b.parts.len());
    let part = |v: &Version, i| v.parts.get(i).copied().unwrap_or(0);
    let mut ord = (0..len)
        .map(|i| part(&a, i).cmp(&part(&b, i)))
        .find(|o| o.is_ne())
        .unwrap_or(Ordering::Equal);
    if ord.is_eq() {
        ord = match (&a.pre, &b.pre) {
            (None, None) => Ordering::Equal,
            (None, Some(_)) => Ordering::Greater,
            (Some(_), None) => Ordering::Less,
            (Some((ta, na)), Some((tb, nb))) => (pre_rank(ta), na).cmp(&(pre_rank(tb), nb)),
        };
    }
    if ord.is_eq() {
        if let (Some(x), Some(y)) = (a.build, b.build) {
            ord = x.cmp(&y);
        }
    }
    match ord {
        Ordering::Less => VersionOrder::Older,
        Ordering::Equal => VersionOrder::Same,
        Ordering::Greater => VersionOrder::Newer,
    }
}

#[cfg(test)]
mod tests {
    use super::VersionOrder::*;
    use super::*;

    #[test]
    fn comparison_table() {
        let cases = [
            ("4.0.2", "4.0.2", Same),
            ("4.02", "4.0.2", Same),
            ("4.0.2", "4.02", Same),
            ("4.0", "4.0.0", Same),
            ("v4.1", "4.1", Same),
            ("Version 2.1", "2.1.0", Same),
            ("4.1", "4.0.9", Newer),
            ("4.10", "4.9", Newer),
            ("4.01", "4.1", Older),
            ("2.1.0.7", "2.1.0.12", Older),
            ("v4 build 1234", "4.0 build 1240", Older),
            ("v4 build 1234", "4.0", Same),
            ("3.0 (1234)", "3.0 (1200)", Newer),
            ("1.4b3", "1.4", Older),
            ("1.4b3", "1.4b2", Newer),
            ("1.4rc1", "1.4beta5", Newer),
            ("1.4", "1.3.9", Newer),
            ("1.4-beta.2", "1.4-beta.1", Newer),
            ("", "1.0", Unknown),
            ("latest", "1.0", Unknown),
            ("LATEST", "latest", Same),
        ];
        for (a, b, expected) in cases {
            assert_eq!(compare_versions(a, b), expected, "{a:?} vs {b:?}");
        }
    }

    #[test]
    fn parses_parts() {
        let v = Version::parse("v4 build 1234").unwrap();
        assert_eq!((v.parts, v.build), (vec![4], Some(1234)));
        let v = Version::parse("1.4b3").unwrap();
        assert_eq!(v.pre, Some(("beta".into(), 3)));
    }
}
