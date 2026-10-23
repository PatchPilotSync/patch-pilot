//! Local SQLite index and content-addressed blob store.
//!
//! The database lives in the OS app-data directory, never in user folders.

/// Manifest schema version this crate was built against.
pub fn schema_version() -> u32 {
    patchpilot_core::MANIFEST_SCHEMA_VERSION
}

#[cfg(test)]
mod tests {
    #[test]
    fn links_against_core() {
        assert_eq!(super::schema_version(), 1);
    }
}
