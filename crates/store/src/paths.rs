use std::path::PathBuf;

use patchpilot_core::Os;

const DB_FILE: &str = "patchpilot.db";

/// Where the database lives: the OS app-data directory, never a user folder.
///
/// - Windows: `%LOCALAPPDATA%\PatchPilot\patchpilot.db` (machine-local, not roaming)
/// - macOS: `~/Library/Application Support/PatchPilot/patchpilot.db`
///
/// `None` if the needed base folder is unknown.
pub fn default_db_path(os: Os, local_app_data: &str, home: &str) -> Option<PathBuf> {
    let base = match os {
        Os::Windows if !local_app_data.is_empty() => PathBuf::from(local_app_data),
        Os::Macos if !home.is_empty() => PathBuf::from(home)
            .join("Library")
            .join("Application Support"),
        _ => return None,
    };
    Some(base.join("PatchPilot").join(DB_FILE))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_data_paths() {
        assert_eq!(
            default_db_path(Os::Macos, "", "/Users/alice").unwrap(),
            PathBuf::from("/Users/alice/Library/Application Support/PatchPilot/patchpilot.db")
        );
        let win = default_db_path(Os::Windows, r"C:\Users\alice\AppData\Local", "").unwrap();
        assert!(win.ends_with("patchpilot.db"));
        assert!(win.to_string_lossy().contains("PatchPilot"));
        assert_eq!(default_db_path(Os::Windows, "", "/Users/alice"), None);
    }
}
