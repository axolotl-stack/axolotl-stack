//! Local immutable resource-pack offers loaded before network admission.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Pack order is also the order advertised to joining clients.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ResourcePackConfig {
    pub paths: Vec<PathBuf>,
    /// Total compressed bytes retained by the listener, shared by all connections.
    pub max_total_bytes: u64,
    /// Maximum decompressed manifest size for each archive.
    pub max_manifest_bytes: u64,
}

impl Default for ResourcePackConfig {
    /// An unconfigured server offers no packs and reserves no archive memory.
    fn default() -> Self {
        Self {
            paths: Vec::new(),
            max_total_bytes: 512 * 1024 * 1024,
            max_manifest_bytes: 1024 * 1024,
        }
    }
}

impl ResourcePackConfig {
    /// Keep byte limits explicit, finite and representable by bounded readers.
    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.max_total_bytes == 0
            || self.max_manifest_bytes == 0
            || self.max_total_bytes >= usize::MAX as u64
            || self.max_manifest_bytes >= usize::MAX as u64
            || self.paths.iter().any(|path| path.as_os_str().is_empty())
        {
            return Err(
                "`resource_packs` requires nonempty paths and positive representable byte limits"
                    .into(),
            );
        }
        Ok(())
    }
}
