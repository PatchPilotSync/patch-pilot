//! Reaper shortcuts between Windows and macOS (decision #81).
//!
//! `reaper-kb.ini` stores a shortcut as `KEY <flags> <key> <action> <section>`
//! with Windows accelerator flags, on both systems: 1 = a virtual key code,
//! 4 = Shift, 8 = Ctrl, 16 = Alt, 32 = Win. Reaper on macOS (through WDL's
//! SWELL) reads Cmd as Ctrl (8), Option as Alt (16) and Control as Win (32),
//! so a PC key map already lands on the most analogous Mac keys: Ctrl+S is
//! Cmd+S. What doesn't carry over is a handful of Cmd shortcuts macOS keeps
//! for itself (Cmd+H hides the app, Cmd+Space opens Spotlight, Cmd+Shift+3
//! takes a screenshot), which Reaper never sees.
//!
//! [`for_mac`] moves each of those to the same key with Control in place of
//! Cmd ([`MAC_SWAPS`]). It is a swap, so applying it twice gives back the
//! file it started from: a Mac keeps its key map moved, shares it in the
//! Windows form (so the same shortcuts hash the same on every machine), and
//! moves what it receives. A Control shortcut there was a Win shortcut on the
//! PC, which Windows mostly keeps for itself, so nothing that worked moves to
//! a key that doesn't.

use std::sync::atomic::{AtomicBool, Ordering};

const VIRTKEY: u32 = 1;
const SHIFT: u32 = 4;
const CTRL: u32 = 8;
const ALT: u32 = 16;
const WIN: u32 = 32;

const TAB: u32 = 0x09;
const ESC: u32 = 0x1B;
const SPACE: u32 = 0x20;

/// A shortcut in `reaper-kb.ini`: its flags and key code.
pub type Shortcut = (u32, u32);

/// Shortcuts macOS keeps for itself (flags, key) and where [`for_mac`] moves
/// them: the same key with Control (Win's flag) for Cmd. Cmd+Space goes to
/// Control+Shift+Space, since Control+Space switches input sources.
pub const MAC_SWAPS: &[(Shortcut, Shortcut)] = &[
    // Hide Reaper; hide other apps.
    ((VIRTKEY | CTRL, b'H' as u32), (VIRTKEY | WIN, b'H' as u32)),
    (
        (VIRTKEY | CTRL | ALT, b'H' as u32),
        (VIRTKEY | WIN | ALT, b'H' as u32),
    ),
    // Switch apps.
    ((VIRTKEY | CTRL, TAB), (VIRTKEY | WIN, TAB)),
    ((VIRTKEY | CTRL | SHIFT, TAB), (VIRTKEY | WIN | SHIFT, TAB)),
    // Spotlight; Finder search.
    ((VIRTKEY | CTRL, SPACE), (VIRTKEY | WIN | SHIFT, SPACE)),
    (
        (VIRTKEY | CTRL | ALT, SPACE),
        (VIRTKEY | WIN | ALT | SHIFT, SPACE),
    ),
    // Screenshots.
    (
        (VIRTKEY | CTRL | SHIFT, b'3' as u32),
        (VIRTKEY | WIN | SHIFT, b'3' as u32),
    ),
    (
        (VIRTKEY | CTRL | SHIFT, b'4' as u32),
        (VIRTKEY | WIN | SHIFT, b'4' as u32),
    ),
    (
        (VIRTKEY | CTRL | SHIFT, b'5' as u32),
        (VIRTKEY | WIN | SHIFT, b'5' as u32),
    ),
    // Force Quit.
    ((VIRTKEY | CTRL | ALT, ESC), (VIRTKEY | WIN | ALT, ESC)),
    // Show or hide the Dock.
    (
        (VIRTKEY | CTRL | ALT, b'D' as u32),
        (VIRTKEY | WIN | ALT, b'D' as u32),
    ),
];

/// Keys a Mac keyboard doesn't have, by Windows virtual key code.
const NOT_ON_MAC: &[(u32, &str)] = &[
    (0x2D, "Insert"),
    (0x13, "Pause"),
    (0x91, "Scroll Lock"),
    (0x2C, "Print Screen"),
    (0x5D, "Menu"),
];

static MAC_FORM: AtomicBool = AtomicBool::new(false);

/// Whether this machine's key map is kept moved for the Mac ([`for_mac`]),
/// so it's shared after moving it back. Set once this Mac's file has been
/// moved (or put back); see `patchpilot_local::mac_keys`.
pub fn set_mac_form(on: bool) {
    MAC_FORM.store(on, Ordering::SeqCst);
}

pub fn mac_form() -> bool {
    MAC_FORM.load(Ordering::SeqCst)
}

/// The `(flags, key)` of a `KEY` line's shortcut, and where those two
/// numbers sit in the line.
fn shortcut(line: &str) -> Option<(Shortcut, std::ops::Range<usize>)> {
    let rest = line.strip_prefix("KEY")?;
    if !rest.starts_with([' ', '\t']) {
        return None;
    }
    let mut fields = Vec::new();
    let mut i = 3;
    for part in rest.split([' ', '\t']) {
        if !part.is_empty() {
            fields.push((i, part));
            if fields.len() == 2 {
                break;
            }
        }
        i += part.len() + 1;
    }
    let [(start, flags), (key_at, key)] = fields[..] else {
        return None;
    };
    let pair = (flags.parse().ok()?, key.parse().ok()?);
    Some((pair, start..key_at + key.len()))
}

fn swapped(pair: Shortcut) -> Option<Shortcut> {
    MAC_SWAPS.iter().find_map(|&(a, b)| {
        if a == pair {
            Some(b)
        } else if b == pair {
            Some(a)
        } else {
            None
        }
    })
}

/// `reaper-kb.ini` text with each shortcut in [`MAC_SWAPS`] swapped with its
/// partner; everything else exactly as it was. Applying it again undoes it.
pub fn for_mac(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        match shortcut(line).and_then(|(pair, at)| Some((swapped(pair)?, at))) {
            Some(((flags, key), at)) => {
                out.push_str(&line[..at.start]);
                out.push_str(&format!("{flags} {key}"));
                out.push_str(&line[at.end..]);
            }
            None => out.push_str(line),
        }
    }
    out
}

/// A shortcut as a Mac shows it: "Cmd+Shift+3".
pub fn mac_label((flags, key): Shortcut) -> String {
    let mut parts: Vec<String> = Vec::new();
    for (bit, name) in [
        (WIN, "Control"),
        (ALT, "Option"),
        (SHIFT, "Shift"),
        (CTRL, "Cmd"),
    ] {
        if flags & bit != 0 {
            parts.push(name.into());
        }
    }
    parts.push(key_name(key));
    parts.join("+")
}

fn key_name(key: u32) -> String {
    match key {
        TAB => "Tab".into(),
        ESC => "Esc".into(),
        SPACE => "Space".into(),
        0x70..=0x87 => format!("F{}", key - 0x6F),
        _ => match NOT_ON_MAC.iter().find(|(k, _)| *k == key) {
            Some((_, name)) => (*name).into(),
            None => char::from_u32(key)
                .filter(char::is_ascii_alphanumeric)
                .map_or_else(|| format!("key {key}"), |c| c.to_string()),
        },
    }
}

/// What a Mac does with a key map shared in the Windows form. Mirrored in
/// `app/src/api.ts`.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct MacShortcuts {
    /// Shortcuts moved off keys macOS keeps: ("Cmd+H", "Control+H", count).
    pub moved: Vec<(String, String, usize)>,
    /// Keys a Mac keyboard doesn't have, and how many shortcuts use each.
    pub missing: Vec<(String, usize)>,
}

/// What [`for_mac`] moves in `text` (the Windows form), and the shortcuts
/// on keys a Mac doesn't have.
pub fn mac_shortcuts(text: &str) -> MacShortcuts {
    let mut report = MacShortcuts::default();
    for line in text.lines() {
        let Some((pair, _)) = shortcut(line) else {
            continue;
        };
        if pair.0 & VIRTKEY == 0 || pair.0 > 0x3F {
            continue;
        }
        if let Some(&(from, to)) = MAC_SWAPS.iter().find(|(from, _)| *from == pair) {
            let (from, to) = (mac_label(from), mac_label(to));
            match report.moved.iter_mut().find(|m| m.0 == from) {
                Some(m) => m.2 += 1,
                None => report.moved.push((from, to, 1)),
            }
        }
        if let Some((_, name)) = NOT_ON_MAC.iter().find(|(k, _)| *k == pair.1) {
            match report.missing.iter_mut().find(|m| m.0 == *name) {
                Some(m) => m.1 += 1,
                None => report.missing.push(((*name).into(), 1)),
            }
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    const PC: &str = "SCR 4 0 RS1 \"Custom: x.lua\" \"C:\\x.lua\"\r\n\
        KEY 9 72 40001 0\r\n\
        KEY 9 72 40002 32060\r\n\
        KEY 9 83 40026 0\r\n\
        KEY 13 51 _SWS_X 0\r\n\
        KEY 9 32 1013 0\r\n\
        KEY 33 72 40003 0\r\n\
        KEY 1 45 40004 0\r\n\
        KEY 255 248 990 0\r\n\
        ACT 0 0 \"x\" \"Custom: y\" 40001\r\n";

    #[test]
    fn swaps_are_partners_once_each() {
        let mut seen = HashSet::new();
        for &(from, to) in MAC_SWAPS {
            assert!(seen.insert(from) && seen.insert(to), "{from:?} {to:?}");
            assert_eq!(from.0 & WIN, 0, "Cmd shortcuts move");
            assert_eq!(to.0 & (WIN | CTRL), WIN, "to Control ones");
            assert_eq!(from.1, to.1, "on the same key");
        }
    }

    #[test]
    fn moves_mac_system_shortcuts_to_control_and_back() {
        let mac = for_mac(PC);
        assert_eq!(
            mac,
            "SCR 4 0 RS1 \"Custom: x.lua\" \"C:\\x.lua\"\r\n\
            KEY 33 72 40001 0\r\n\
            KEY 33 72 40002 32060\r\n\
            KEY 9 83 40026 0\r\n\
            KEY 37 51 _SWS_X 0\r\n\
            KEY 37 32 1013 0\r\n\
            KEY 9 72 40003 0\r\n\
            KEY 1 45 40004 0\r\n\
            KEY 255 248 990 0\r\n\
            ACT 0 0 \"x\" \"Custom: y\" 40001\r\n"
        );
        assert_eq!(for_mac(&mac), PC);
    }

    #[test]
    fn leaves_other_lines_alone() {
        for text in [
            "",
            "KEY",
            "KEY x 72 1 0\n",
            "KEYS 9 72 1 0",
            "KEY 9\n",
            "  KEY 9 72 1 0\n",
        ] {
            assert_eq!(for_mac(text), text);
        }
        assert_eq!(for_mac("KEY\t9  72 1 0"), "KEY\t33 72 1 0");
    }

    #[test]
    fn says_what_moves_and_what_a_mac_lacks() {
        let r = mac_shortcuts(PC);
        assert_eq!(
            r.moved,
            vec![
                ("Cmd+H".into(), "Control+H".into(), 2),
                ("Shift+Cmd+3".into(), "Control+Shift+3".into(), 1),
                ("Cmd+Space".into(), "Control+Shift+Space".into(), 1),
            ]
        );
        assert_eq!(r.missing, vec![("Insert".into(), 1)]);
    }
}
