use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{Item, ItemKind, PluginFormat, PortablePath, Product, ProductId};

/// A Primary Rig narrowed for one machine, e.g. "Travel: no AAX, no
/// libraries over 20 GB".
///
/// Rules are checked in order and the **last** matching rule wins, like
/// `.gitignore`; an item no rule matches is included. So an `Exclude` can be
/// followed by a narrower `Include` to carve out an exception.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TargetProfile {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub rules: Vec<ProfileRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProfileRule {
    pub action: RuleAction,
    #[serde(rename = "match")]
    pub matcher: Matcher,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RuleAction {
    Include,
    Exclude,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Matcher {
    /// Product vendor, case-insensitive.
    Vendor(String),
    Format(PluginFormat),
    Kind(ItemKind),
    Product(ProductId),
    /// Items larger than this many bytes. Items with no known size never match.
    SizeOver(u64),
    /// Glob over the item's path written `ROOT/rel` (logical) or as-is
    /// (absolute), case-insensitive. `*` and `?` stay within one `/`-separated
    /// segment; `**` spans segments.
    PathGlob(String),
}

impl TargetProfile {
    /// Whether `item` (of `product`, if known) is part of this target.
    pub fn includes(&self, item: &Item, product: Option<&Product>) -> bool {
        self.rules
            .iter()
            .rev()
            .find(|r| r.matcher.matches(item, product))
            .is_none_or(|r| r.action == RuleAction::Include)
    }
}

impl Matcher {
    pub fn matches(&self, item: &Item, product: Option<&Product>) -> bool {
        match self {
            Self::Vendor(v) => product.is_some_and(|p| p.vendor.eq_ignore_ascii_case(v)),
            Self::Format(f) => item.format == Some(*f),
            Self::Kind(k) => item.kind == *k,
            Self::Product(id) => item.product_id.as_ref() == Some(id),
            Self::SizeOver(n) => item.size.is_some_and(|s| s > *n),
            Self::PathGlob(g) => {
                glob_match(&g.to_lowercase(), &path_string(&item.path).to_lowercase())
            }
        }
    }
}

fn path_string(p: &PortablePath) -> String {
    match p {
        PortablePath::Logical { root, rel } if rel.is_empty() => root.clone(),
        PortablePath::Logical { root, rel } => format!("{root}/{rel}"),
        PortablePath::Absolute(a) => a.replace('\\', "/"),
    }
}

/// Minimal glob: `**` (any segments), `*` (within a segment), `?` (one char).
fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    glob_from(&p, &t)
}

fn glob_from(p: &[char], t: &[char]) -> bool {
    match p {
        [] => t.is_empty(),
        ['*', '*', rest @ ..] => {
            let rest = rest.strip_prefix(&['/']).unwrap_or(rest);
            (0..=t.len()).any(|i| glob_from(rest, &t[i..]))
        }
        ['*', rest @ ..] => {
            let seg_end = t.iter().position(|&c| c == '/').unwrap_or(t.len());
            (0..=seg_end).any(|i| glob_from(rest, &t[i..]))
        }
        ['?', rest @ ..] => t.first().is_some_and(|&c| c != '/') && glob_from(rest, &t[1..]),
        [c, rest @ ..] => t.first() == Some(c) && glob_from(rest, &t[1..]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Os;

    #[test]
    fn globs() {
        assert!(glob_match("vst3_system/*.vst3", "vst3_system/verb.vst3"));
        assert!(!glob_match(
            "vst3_system/*.vst3",
            "vst3_system/acme/verb.vst3"
        ));
        assert!(glob_match(
            "vst3_system/**/*.vst3",
            "vst3_system/acme/verb.vst3"
        ));
        assert!(glob_match("vst3_system/**/*.vst3", "vst3_system/verb.vst3"));
        assert!(glob_match("samples_root/**", "samples_root/a/b/c.wav"));
        assert!(glob_match("a?c", "abc"));
        assert!(!glob_match("a?c", "a/c"));
    }

    fn item(format: PluginFormat, size: Option<u64>) -> Item {
        let mut i = Item::plugin(
            format,
            Os::Windows,
            PortablePath::logical("VST3_SYSTEM", "Acme/Verb.vst3"),
        );
        i.size = size;
        i
    }

    #[test]
    fn last_matching_rule_wins() {
        let profile = TargetProfile {
            id: "travel".into(),
            name: "Travel".into(),
            rules: vec![
                ProfileRule {
                    action: RuleAction::Exclude,
                    matcher: Matcher::Format(PluginFormat::Aax),
                },
                ProfileRule {
                    action: RuleAction::Exclude,
                    matcher: Matcher::SizeOver(20_000_000_000),
                },
                ProfileRule {
                    action: RuleAction::Include,
                    matcher: Matcher::PathGlob("VST3_SYSTEM/Acme/**".into()),
                },
            ],
        };
        let aax = Item {
            path: PortablePath::logical("AAX_SYSTEM", "Verb.aaxplugin"),
            ..item(PluginFormat::Aax, None)
        };
        assert!(!profile.includes(&aax, None));
        // Big, but the later Include for Acme's folder wins.
        assert!(profile.includes(&item(PluginFormat::Vst3, Some(30_000_000_000)), None));
        let big_elsewhere = Item {
            path: PortablePath::logical("SAMPLES_ROOT", "Huge Library"),
            ..item(PluginFormat::Vst3, Some(30_000_000_000))
        };
        assert!(!profile.includes(&big_elsewhere, None));
        assert!(TargetProfile::default().includes(&aax, None));
    }

    #[test]
    fn json_shape() {
        let rule: ProfileRule =
            serde_json::from_str(r#"{ "action": "exclude", "match": { "format": "aax" } }"#)
                .unwrap();
        assert_eq!(rule.matcher, Matcher::Format(PluginFormat::Aax));
    }
}
