//! Which DAWs are running, so Patch Pilot never writes a DAW's settings
//! while it is open (CLAUDE.md rule 5).

use std::process::Command;

use patchpilot_parity::ableton::{ABLETON_PREFS, ABLETON_USER_LIBRARY};

/// Ok when files under logical `root` may be written now; otherwise a
/// message for the user. Only Ableton roots are guarded here.
pub fn write_guard(root: &str) -> Result<(), String> {
    if (root == ABLETON_USER_LIBRARY || root == ABLETON_PREFS) && ableton_live_running() {
        return Err("Close Ableton Live first, then try again.".to_owned());
    }
    Ok(())
}

/// True while any version of Ableton Live is open. If the process list
/// can't be read, assumes it is, so nothing is written by mistake.
pub fn ableton_live_running() -> bool {
    match process_list() {
        Some(list) => lists_ableton_live(&list),
        None => true,
    }
}

/// Spot Live in `tasklist /FO CSV /NH` (Windows: `"Ableton Live 12
/// Suite.exe","1234",...`) or `ps -axo comm=` (macOS:
/// `/Applications/Ableton Live 12 Suite.app/Contents/MacOS/Live`).
pub fn lists_ableton_live(list: &str) -> bool {
    list.lines().any(|line| {
        let line = line.trim();
        let exe = line
            .strip_prefix('"')
            .and_then(|l| l.split('"').next())
            .unwrap_or(line);
        let lower = exe.to_ascii_lowercase();
        const MAC_EXE: &str = ".app/contents/macos/live";
        let mac = lower.find(MAC_EXE).is_some_and(|at| {
            lower[..at].contains("/ableton live")
                && lower[at + MAC_EXE.len()..]
                    .chars()
                    .next()
                    .is_none_or(char::is_whitespace)
        });
        (lower.starts_with("ableton live") && lower.ends_with(".exe")) || mac
    })
}

#[cfg(windows)]
fn process_list() -> Option<String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let out = Command::new("tasklist")
        .args(["/FO", "CSV", "/NH"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(not(windows))]
fn process_list() -> Option<String> {
    let out = Command::new("ps").args(["-axo", "comm="]).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_live_in_process_lists() {
        let win = "\"explorer.exe\",\"4120\",\"Console\",\"1\",\"80,312 K\"\r\n\
                   \"Ableton Live 12 Suite.exe\",\"9876\",\"Console\",\"1\",\"1,204,000 K\"\r\n";
        assert!(lists_ableton_live(win));
        let mac = "/usr/sbin/cfprefsd\n\
                   /Applications/Ableton Live 12 Suite.app/Contents/MacOS/Live\n";
        assert!(lists_ableton_live(mac));

        let neither = "\"Ableton Index.exe\",\"1\"\n\
                       /Applications/Ableton Live 12 Suite.app/Contents/MacOS/Ableton Index\n\
                       /Applications/Patch Pilot.app/Contents/MacOS/patch-pilot\n";
        assert!(!lists_ableton_live(neither));
    }

    #[test]
    fn only_ableton_roots_are_guarded() {
        assert_eq!(write_guard("REAPER_RESOURCES"), Ok(()));
    }
}
