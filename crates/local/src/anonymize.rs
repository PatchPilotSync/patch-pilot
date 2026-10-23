//! Strip usernames from paths before a manifest is shared.

use patchpilot_core::{Manifest, PortablePath};

/// Replace the home folder with `~` and any other path segment equal to the
/// username with `<user>`, in the path map and in absolute item paths.
///
/// Logical paths (`VST3_SYSTEM/Acme/Verb.vst3`) carry no machine-specific
/// prefix, so only their `rel` part is checked for the username.
pub fn anonymize(m: &mut Manifest, home: Option<&str>, user: Option<&str>) {
    let fix = |s: &str| scrub(s, home, user);
    for physical in m.machine.path_map.roots.values_mut() {
        *physical = fix(physical);
    }
    for item in &mut m.items {
        match &mut item.path {
            PortablePath::Absolute(a) => *a = fix(a),
            PortablePath::Logical { rel, .. } => *rel = scrub(rel, None, user),
        }
    }
}

fn scrub(s: &str, home: Option<&str>, user: Option<&str>) -> String {
    let mut out = s.to_owned();
    if let Some(home) = home
        .map(|h| h.trim_end_matches(['/', '\\']))
        .filter(|h| h.len() > 1)
    {
        let starts = out
            .get(..home.len())
            .is_some_and(|p| p.eq_ignore_ascii_case(home));
        let at_boundary = out[home.len().min(out.len())..]
            .chars()
            .next()
            .is_none_or(|c| matches!(c, '/' | '\\'));
        if starts && at_boundary {
            out = format!("~{}", &out[home.len()..]);
        }
    }
    if let Some(user) = user.filter(|u| !u.is_empty()) {
        out = out
            .split_inclusive(['/', '\\'])
            .map(|seg| {
                let (name, sep) = match seg.strip_suffix(['/', '\\']) {
                    Some(n) => (n, &seg[n.len()..]),
                    None => (seg, ""),
                };
                if name.eq_ignore_ascii_case(user) {
                    format!("<user>{sep}")
                } else {
                    seg.to_owned()
                }
            })
            .collect();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrubs_home_and_username() {
        let h = Some(r"C:\Users\alice");
        let u = Some("alice");
        assert_eq!(
            scrub(r"C:\Users\alice\AppData\Roaming\REAPER", h, u),
            r"~\AppData\Roaming\REAPER"
        );
        assert_eq!(scrub(r"c:\users\ALICE\x", h, u), r"~\x");
        assert_eq!(
            scrub(r"D:\Backups\alice\VST", h, u),
            r"D:\Backups\<user>\VST"
        );
        assert_eq!(
            scrub("/Users/alice/Library/Audio", Some("/Users/alice"), u),
            "~/Library/Audio"
        );
        // Not a whole segment: left alone.
        assert_eq!(
            scrub("/Users/alicea/x", Some("/Users/alice"), u),
            "/Users/alicea/x"
        );
        assert_eq!(
            scrub("/Library/Audio/Plug-Ins/VST3", Some("/Users/alice"), u),
            "/Library/Audio/Plug-Ins/VST3"
        );
    }
}
