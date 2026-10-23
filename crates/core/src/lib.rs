//! Shared domain types for Patch Pilot: `Item`, `Product`, `Manifest`, `Snapshot`, `Profile`.
//!
//! Types land in #4. Changes here are reviewed by both founders.

/// Version of the manifest JSON format this build reads and writes.
pub const MANIFEST_SCHEMA_VERSION: u32 = 1;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_version_starts_at_one() {
        assert_eq!(MANIFEST_SCHEMA_VERSION, 1);
    }
}
