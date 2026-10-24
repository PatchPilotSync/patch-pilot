//! Who signed an installer, so the vault can warn when a new version of a
//! product comes from a different signer (strategy section 14: installers
//! are a malware vector).
//!
//! Asks the OS's own tools in a subprocess and only reads their output:
//! `pkgutil --check-signature` for `.pkg`, `codesign -dv` for `.dmg` on
//! macOS, PowerShell's `Get-AuthenticodeSignature` for `.exe`/`.msi` on
//! Windows. Nothing is run from the installer itself. Unsigned files, `.zip`
//! files and failures all read as "no signer".

use std::path::Path;
use std::process::Command;

/// The signer's name, e.g. `FabFilter B.V. (5A4X8M7KQT)` or `FabFilter B.V.`,
/// or `None` if the file isn't signed or the OS couldn't tell.
pub fn signer(path: &Path) -> Option<String> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    if cfg!(target_os = "macos") {
        match ext.as_str() {
            "pkg" | "mpkg" => {
                let out = Command::new("/usr/sbin/pkgutil")
                    .arg("--check-signature")
                    .arg(path)
                    .output()
                    .ok()?;
                parse_pkgutil(&String::from_utf8_lossy(&out.stdout))
            }
            "dmg" => {
                let out = Command::new("/usr/bin/codesign")
                    .args(["-dv", "--verbose=2"])
                    .arg(path)
                    .output()
                    .ok()?;
                // codesign reports on stderr.
                parse_codesign(&String::from_utf8_lossy(&out.stderr))
            }
            _ => None,
        }
    } else if cfg!(windows) {
        match ext.as_str() {
            "exe" | "msi" => windows_signer(path),
            _ => None,
        }
    } else {
        None
    }
}

#[cfg(windows)]
fn windows_signer(path: &Path) -> Option<String> {
    use std::os::windows::process::CommandExt;
    // No console window flashing up.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    // The path goes in through an environment variable, never into the
    // command text, so no file name can change what runs.
    let script = "$s = Get-AuthenticodeSignature -LiteralPath $env:PATCHPILOT_FILE; \
                  \"$($s.Status)|$($s.SignerCertificate.Subject)\"";
    let out = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .env("PATCHPILOT_FILE", path)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    parse_authenticode(&String::from_utf8_lossy(&out.stdout))
}

#[cfg(not(windows))]
fn windows_signer(_path: &Path) -> Option<String> {
    None
}

/// The first certificate in `pkgutil --check-signature` output, minus its
/// "Developer ID Installer: " prefix.
pub fn parse_pkgutil(out: &str) -> Option<String> {
    if !out.contains("Status: signed") {
        return None;
    }
    let first = out
        .lines()
        .map(str::trim)
        .find_map(|l| l.strip_prefix("1. "))?;
    Some(strip_role(first).to_owned())
}

/// The first `Authority=` line of `codesign -dv --verbose=2`.
pub fn parse_codesign(out: &str) -> Option<String> {
    let first = out
        .lines()
        .map(str::trim)
        .find_map(|l| l.strip_prefix("Authority="))?;
    Some(strip_role(first).to_owned())
}

/// `Valid|CN=FabFilter B.V., O=FabFilter B.V., C=NL` → `FabFilter B.V.`.
/// Anything but a valid signature (unsigned, hash mismatch, untrusted) is
/// `None`.
pub fn parse_authenticode(out: &str) -> Option<String> {
    let (status, subject) = out.trim().split_once('|')?;
    if status.trim() != "Valid" {
        return None;
    }
    let cn = subject
        .split(", ")
        .find_map(|part| part.trim().strip_prefix("CN="))?;
    Some(cn.trim_matches('"').to_owned())
}

fn strip_role(s: &str) -> &str {
    s.split_once(": ").map_or(s, |(_, name)| name).trim()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_pkgutil_output() {
        let out = "Package \"Pro-Q 3 Installer.pkg\":\n   Status: signed by a developer certificate issued by Apple for distribution\n   Notarization: trusted by the Apple notary service\n   Signed with a trusted timestamp on: 2024-03-01 10:00:00 +0000\n   Certificate Chain:\n    1. Developer ID Installer: FabFilter B.V. (5A4X8M7KQT)\n       Expires: 2027-02-01 22:12:15 +0000\n    2. Developer ID Certification Authority\n";
        assert_eq!(
            parse_pkgutil(out).as_deref(),
            Some("FabFilter B.V. (5A4X8M7KQT)")
        );
        assert_eq!(
            parse_pkgutil("Package \"x.pkg\":\n   Status: no signature\n"),
            None
        );
    }

    #[test]
    fn reads_codesign_output() {
        let out = "Executable=/Volumes/x\nIdentifier=Diva\nAuthority=Developer ID Application: u-he (ABCDE12345)\nAuthority=Developer ID Certification Authority\n";
        assert_eq!(parse_codesign(out).as_deref(), Some("u-he (ABCDE12345)"));
        assert_eq!(
            parse_codesign("x.dmg: code object is not signed at all\n"),
            None
        );
    }

    #[test]
    fn reads_authenticode_output() {
        let out = "Valid|CN=FabFilter B.V., O=FabFilter B.V., L=Amsterdam, C=NL\r\n";
        assert_eq!(parse_authenticode(out).as_deref(), Some("FabFilter B.V."));
        assert_eq!(parse_authenticode("NotSigned|\r\n"), None);
        assert_eq!(parse_authenticode("HashMismatch|CN=Someone, O=X\r\n"), None);
    }
}
