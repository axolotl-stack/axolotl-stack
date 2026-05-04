//! Source metadata for generated gameplay data.
//!
//! `unastar-data-gen` writes `output/manifest.kdl` next to the golden KDL
//! artifacts. Keeping the raw manifest available at runtime gives diagnostics
//! and bug reports a stable way to identify the exact source inputs that shaped
//! generated gameplay tables.

/// Raw KDL source manifest emitted by `unastar-data-gen`.
pub const SOURCE_MANIFEST_KDL: &str = include_str!("../output/manifest.kdl");

/// Current manifest schema version expected by this crate.
pub const SOURCE_MANIFEST_SCHEMA_VERSION: &str = "1";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_is_embedded() {
        assert!(SOURCE_MANIFEST_KDL.contains("manifest"));
        assert!(SOURCE_MANIFEST_KDL.contains("schema_version=\"1\""));
    }
}
