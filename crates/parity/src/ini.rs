//! Just enough INI for Reaper's `reaper.ini` and ReaPack's `reapack.ini`:
//! read keys, and change a few of them while keeping every other line, the
//! order and the line endings exactly as they were.

/// The `key=value` lines of one section, in file order. Section and key
/// names match ASCII case-insensitively, as Windows' INI functions do.
pub fn section<'a>(text: &'a str, name: &str) -> Vec<(&'a str, &'a str)> {
    let mut out = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        let line = line.trim();
        if let Some(header) = header(line) {
            inside = header.eq_ignore_ascii_case(name);
        } else if inside && let Some((k, v)) = line.split_once('=') {
            out.push((k.trim(), v.trim()));
        }
    }
    out
}

/// The value of `key` in `section`, if set.
pub fn get<'a>(text: &'a str, section_name: &str, key: &str) -> Option<&'a str> {
    section(text, section_name)
        .into_iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, v)| v)
}

/// `text` with each `(key, value)` set in `section_name`: an existing line is
/// replaced in place, a new key goes at the end of the section, and a missing
/// section is added at the end of the file.
pub fn set(text: &str, section_name: &str, values: &[(String, String)]) -> String {
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    let mut pending: Vec<&(String, String)> = values.iter().collect();

    let start = lines
        .iter()
        .position(|l| header(l.trim()).is_some_and(|h| h.eq_ignore_ascii_case(section_name)));
    let Some(start) = start else {
        if lines.last().is_some_and(|l| !l.trim().is_empty()) {
            lines.push(String::new());
        }
        lines.push(format!("[{section_name}]"));
        lines.extend(values.iter().map(|(k, v)| format!("{k}={v}")));
        return join(lines, eol);
    };
    let end = lines[start + 1..]
        .iter()
        .position(|l| header(l.trim()).is_some())
        .map_or(lines.len(), |n| start + 1 + n);

    for line in &mut lines[start + 1..end] {
        let Some((k, _)) = line.split_once('=') else {
            continue;
        };
        let k = k.trim();
        if let Some(n) = pending
            .iter()
            .position(|(pk, _)| pk.eq_ignore_ascii_case(k))
        {
            let (pk, pv) = pending.remove(n);
            *line = format!("{pk}={pv}");
        }
    }
    // New keys go after the section's last non-blank line.
    let mut at = end;
    while at > start + 1 && lines[at - 1].trim().is_empty() {
        at -= 1;
    }
    for (k, v) in pending.into_iter().rev() {
        lines.insert(at, format!("{k}={v}"));
    }
    join(lines, eol)
}

fn header(line: &str) -> Option<&str> {
    line.strip_prefix('[')?.strip_suffix(']').map(str::trim)
}

fn join(lines: Vec<String>, eol: &str) -> String {
    let mut out = lines.join(eol);
    out.push_str(eol);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const INI: &str = "[REAPER]\r\ndefrecpath=D:\\Rec\r\nfoo=1\r\n\r\n[reaper_explorer]\r\nShortcut0=D:\\Samples\r\n";

    #[test]
    fn reads_keys_case_insensitively() {
        assert_eq!(get(INI, "reaper", "DefRecPath"), Some("D:\\Rec"));
        assert_eq!(get(INI, "REAPER", "missing"), None);
        assert_eq!(
            section(INI, "reaper_explorer"),
            [("Shortcut0", "D:\\Samples")]
        );
    }

    #[test]
    fn replaces_and_appends_keeping_everything_else() {
        let out = set(
            INI,
            "REAPER",
            &[
                ("defrecpath".into(), "E:\\Rec".into()),
                ("altpeakspath".into(), "E:\\Peaks".into()),
            ],
        );
        assert_eq!(
            out,
            "[REAPER]\r\ndefrecpath=E:\\Rec\r\nfoo=1\r\naltpeakspath=E:\\Peaks\r\n\r\n[reaper_explorer]\r\nShortcut0=D:\\Samples\r\n"
        );
    }

    #[test]
    fn adds_a_missing_section() {
        let out = set("[a]\nx=1\n", "remotes", &[("size".into(), "1".into())]);
        assert_eq!(out, "[a]\nx=1\n\n[remotes]\nsize=1\n");
    }
}
