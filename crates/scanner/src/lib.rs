//! Plug-in, DAW and vendor-app scanners.
//!
//! Metadata only: never load a plug-in binary in-process (CLAUDE.md rule 1).

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
