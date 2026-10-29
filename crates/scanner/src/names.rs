//! Clean up plug-in names so variants of one product read as one name.
//!
//! Vendors and users decorate file names with architecture and channel tags:
//! `Acme Comp x64`, `Acme Gate (64 bit)`, `Acme EQ Stereo`, `Acme Verb (m/s)`.
//! These are stripped from the end of the name and returned separately.

use patchpilot_core::Arch;

/// Channel layout a name says a variant is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channels {
    Mono,
    Stereo,
    MonoStereo,
    Surround,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedName {
    /// The product name with trailing tags removed, original case kept.
    pub name: String,
    pub channels: Option<Channels>,
    /// Set for 64-bit tags. 32-bit tags are stripped but not recorded, since
    /// [`Arch`] has no 32-bit value; the PE header is the real source.
    pub arch: Option<Arch>,
}

enum Tag {
    Arch(Option<Arch>),
    Channels(Channels),
    /// `VST`, `VST3`, `AAX`, ...: the format is known from the file anyway.
    Format,
}

/// Strip trailing architecture, channel and format tags.
///
/// Stops at the first trailing word it doesn't recognise, and never strips
/// the whole name. A channel word stays when all that's left before it is a
/// short vendor prefix: Kilohearts' `kHs Stereo` is a plug-in called Stereo,
/// not a stereo `kHs`.
pub fn normalize_name(raw: &str) -> NormalizedName {
    let mut name = trim_seps(raw);
    let mut out = NormalizedName {
        name: String::new(),
        channels: None,
        arch: None,
    };
    while let Some((rest, tag)) = strip_one(name) {
        let rest = trim_seps(rest);
        if rest.is_empty() || (matches!(tag, Tag::Channels(_)) && is_prefix_only(rest)) {
            break;
        }
        match tag {
            Tag::Arch(a) => out.arch = out.arch.or(a),
            Tag::Channels(c) => out.channels = out.channels.or(Some(c)),
            Tag::Format => {}
        }
        name = rest;
    }
    out.name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    out
}

/// One word of at most three characters (`kHs`, `bx`): too short to be a
/// product name on its own.
fn is_prefix_only(s: &str) -> bool {
    !s.contains(char::is_whitespace) && s.chars().count() <= 3
}

fn trim_seps(s: &str) -> &str {
    s.trim_matches(|c: char| c.is_whitespace() || matches!(c, '_' | '-' | '.'))
}

/// Remove one trailing tag: a bracketed group, or a final word.
fn strip_one(s: &str) -> Option<(&str, Tag)> {
    if let Some(close) = s.chars().last().filter(|c| matches!(c, ')' | ']')) {
        let open = if close == ')' { '(' } else { '[' };
        let start = s.rfind(open)?;
        let inner = &s[start + 1..s.len() - 1];
        return classify(inner, true).map(|t| (&s[..start], t));
    }
    let is_sep = |c: char| c.is_whitespace() || matches!(c, '_' | '-');
    let start = s.rfind(is_sep).map_or(0, |i| i + 1);
    let last = &s[start..];
    // "64 bit" / "64-bit" are two words.
    if last.eq_ignore_ascii_case("bit") {
        let before = trim_seps(&s[..start]);
        let num_start = before.rfind(is_sep).map_or(0, |i| i + 1);
        let tag = classify(&format!("{}bit", &before[num_start..]), false)?;
        return Some((&before[..num_start], tag));
    }
    classify(last, false).map(|t| (&s[..start], t))
}

fn classify(token: &str, bracketed: bool) -> Option<Tag> {
    let t: String = token
        .chars()
        .filter(|c| !c.is_whitespace() && !matches!(c, '-' | '_'))
        .collect::<String>()
        .to_ascii_lowercase();
    let tag = match t.as_str() {
        "x64" | "64bit" | "x8664" | "amd64" | "win64" => Tag::Arch(Some(Arch::X86_64)),
        "arm64" | "aarch64" => Tag::Arch(Some(Arch::Aarch64)),
        "x86" | "32bit" | "win32" => Tag::Arch(None),
        "mono" => Tag::Channels(Channels::Mono),
        "stereo" => Tag::Channels(Channels::Stereo),
        "mono/stereo" | "monostereo" | "mono>stereo" => Tag::Channels(Channels::MonoStereo),
        "surround" => Tag::Channels(Channels::Surround),
        "vst" | "vst2" | "vst3" | "aax" | "au" | "clap" => Tag::Format,
        // Short forms are too ambiguous outside brackets ("Acme 64", "Model S").
        "64" if bracketed => Tag::Arch(Some(Arch::X86_64)),
        "32" if bracketed => Tag::Arch(None),
        "m" if bracketed => Tag::Channels(Channels::Mono),
        "s" if bracketed => Tag::Channels(Channels::Stereo),
        "m/s" | "ms" if bracketed => Tag::Channels(Channels::MonoStereo),
        _ => return None,
    };
    Some(tag)
}

#[cfg(test)]
mod tests {
    use super::*;
    use Channels::*;

    #[test]
    fn normalization_table() {
        let x64 = Some(Arch::X86_64);
        let cases: &[(&str, &str, Option<Channels>, Option<Arch>)] = &[
            ("Acme Comp", "Acme Comp", None, None),
            ("Acme Comp x64", "Acme Comp", None, x64),
            ("Acme Comp_x64", "Acme Comp", None, x64),
            ("Acme Comp (x64)", "Acme Comp", None, x64),
            ("Acme Gate (64 bit)", "Acme Gate", None, x64),
            ("Acme Gate (64-bit)", "Acme Gate", None, x64),
            ("Acme Gate 64-bit", "Acme Gate", None, x64),
            ("Acme Gate 64bit", "Acme Gate", None, x64),
            ("Acme Gate (32 bit)", "Acme Gate", None, None),
            ("Acme EQ Mono", "Acme EQ", Some(Mono), None),
            ("Acme EQ Stereo", "Acme EQ", Some(Stereo), None),
            ("Acme EQ (Mono/Stereo)", "Acme EQ", Some(MonoStereo), None),
            ("Acme Verb (m)", "Acme Verb", Some(Mono), None),
            ("Acme Verb (s)", "Acme Verb", Some(Stereo), None),
            ("Acme Verb Stereo x64", "Acme Verb", Some(Stereo), x64),
            ("Acme Verb x64 (Stereo)", "Acme Verb", Some(Stereo), x64),
            ("Acme Verb VST3", "Acme Verb", None, None),
            ("Acme Verb [AAX]", "Acme Verb", None, None),
            ("Acme Verb Surround", "Acme Verb", Some(Surround), None),
            ("  Acme   Verb  ", "Acme Verb", None, None),
            // Not tags: keep as part of the name.
            ("Acme Model S", "Acme Model S", None, None),
            ("Acme 64", "Acme 64", None, None),
            ("Acme Verb (Lite)", "Acme Verb (Lite)", None, None),
            ("Acme Verb Mono Lite", "Acme Verb Mono Lite", None, None),
            // A vendor prefix and a channel word is a product name.
            ("kHs Stereo", "kHs Stereo", None, None),
            ("kHs Stereo x64", "kHs Stereo", None, x64),
            // Never strip to nothing.
            ("Stereo", "Stereo", None, None),
            ("x64", "x64", None, None),
        ];
        for &(raw, name, channels, arch) in cases {
            let n = normalize_name(raw);
            assert_eq!(
                (n.name.as_str(), n.channels, n.arch),
                (name, channels, arch),
                "{raw:?}"
            );
        }
    }
}
