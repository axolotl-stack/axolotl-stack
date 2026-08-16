//! Block registry for runtime block management.
//!
//! Blocks are more complex than items/entities because of block states.
//! Each block type has multiple runtime IDs (one per state combination).

use super::RegistryEntry;
use std::collections::HashMap;
use std::sync::LazyLock;

const MISSING_INDEX: usize = usize::MAX;

static VANILLA_BLOCK_REGISTRY: LazyLock<BlockRegistry> = LazyLock::new(|| {
    let mut registry = BlockRegistry::new();
    registry.load_vanilla();
    registry
});

/// Runtime block entry in the registry.
#[derive(Debug, Clone)]
pub struct BlockEntry {
    /// Legacy numeric block ID.
    ///
    /// Bedrock has duplicate legacy IDs; use string ID or runtime state ID for
    /// exact identity.
    pub id: u32,
    /// String identifier (e.g., "minecraft:stone").
    pub string_id: String,
    /// Display name.
    pub name: String,
    /// Number of known canonical state variants.
    ///
    /// This currently mirrors `state_id_count` because the normalized block
    /// artifact intentionally treats canonical runtime ranges as the source of
    /// truth until richer native state metadata exists.
    pub state_count: u32,
    /// Number of canonical runtime state IDs in this block's range.
    pub state_id_count: u32,
    /// Minimum runtime state ID (from canonical block states).
    pub min_state_id: u32,
    /// Maximum runtime state ID (from canonical block states).
    pub max_state_id: u32,
    /// Default state ID for this block.
    pub default_state_id: u32,
    /// Bootstrap hardness value from the normalized block artifact.
    pub hardness: f32,
    /// Bootstrap explosion resistance value from the normalized block artifact.
    pub resistance: f32,
    /// Bootstrap lighting/render transparency from the normalized block artifact.
    pub is_transparent: bool,
    /// Bootstrap emitted light level from the normalized block artifact.
    pub emit_light: u8,
    /// Bootstrap filtered light level from the normalized block artifact.
    pub filter_light: u8,
}

impl RegistryEntry for BlockEntry {
    fn id(&self) -> u32 {
        self.id
    }

    fn string_id(&self) -> &str {
        &self.string_id
    }
}

/// Block registry with O(1) runtime_id -> block lookup.
#[derive(Debug, Clone, Default)]
pub struct BlockRegistry {
    entries: Vec<BlockEntry>,
    /// Maps legacy numeric block ID to the first matching entry.
    ///
    /// Bedrock has duplicate numeric IDs in generated data, so string ID or
    /// runtime state ID lookups are preferred for precise block identity.
    id_map: HashMap<u32, usize>,
    /// Maps string ID to entry index.
    name_map: HashMap<String, usize>,
    /// Maps runtime state ID to entry index. Index = runtime_id.
    runtime_id_map: Vec<usize>,
}

impl BlockRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Shared vanilla-only, read-only registry snapshot for systems that need
    /// protocol block IDs outside ECS resource access.
    ///
    /// Runtime-mutated or plugin-provided block registries should flow through
    /// explicit `BlockRegistry` resources instead of this global snapshot.
    pub fn vanilla() -> &'static Self {
        &VANILLA_BLOCK_REGISTRY
    }

    /// Load vanilla blocks from the normalized block artifact.
    ///
    /// The artifact currently bootstraps canonical runtime ranges and physical
    /// fields from Valentine, but keeps that weak source boundary in
    /// `unastar-data` so BDS/native facts can replace it without touching
    /// registry consumers.
    pub fn load_vanilla(&mut self) {
        use unastar_data::blocks::ALL_BLOCKS;

        self.entries.clear();
        self.id_map.clear();
        self.name_map.clear();

        // Find max runtime_id to size the lookup table.
        let max_rid = ALL_BLOCKS
            .iter()
            .map(|block| block.max_state_id)
            .max()
            .unwrap_or(0);
        self.runtime_id_map = vec![MISSING_INDEX; (max_rid + 1) as usize];

        for block in ALL_BLOCKS.iter() {
            let entry = BlockEntry {
                id: block.legacy_id,
                string_id: block.identifier.to_string(),
                name: block.name.to_string(),
                state_count: block.state_id_count,
                state_id_count: block.state_id_count,
                min_state_id: block.min_state_id,
                max_state_id: block.max_state_id,
                default_state_id: block.default_state_id,
                hardness: block.hardness,
                resistance: block.resistance,
                is_transparent: block.is_transparent,
                emit_light: block.emit_light,
                filter_light: block.filter_light,
            };

            let entry_index = self.entries.len();

            // Fill the runtime_id -> entry mapping for every state.
            for rid in entry.min_state_id..=entry.max_state_id {
                self.runtime_id_map[rid as usize] = entry_index;
            }

            // Keep the first block for legacy numeric-ID lookup, but always
            // preserve every block by string ID and runtime state ID.
            self.id_map.entry(entry.id).or_insert(entry_index);
            self.name_map.insert(entry.string_id.clone(), entry_index);
            self.entries.push(entry);
        }
    }

    /// Get block entry by runtime ID (state ID). O(1).
    #[inline]
    pub fn get_by_runtime_id(&self, runtime_id: u32) -> Option<&BlockEntry> {
        let entry_index = *self.runtime_id_map.get(runtime_id as usize)?;
        if entry_index == MISSING_INDEX {
            return None;
        }
        self.entries.get(entry_index)
    }

    /// Get entry by block ID. O(1).
    ///
    /// Prefer `get_by_name` or `get_by_runtime_id` when exact identity matters,
    /// because Bedrock block numeric IDs are not globally unique.
    #[inline]
    pub fn get(&self, id: u32) -> Option<&BlockEntry> {
        self.id_map
            .get(&id)
            .and_then(|index| self.entries.get(*index))
    }

    /// Get entry by string ID. O(1).
    pub fn get_by_name(&self, name: &str) -> Option<&BlockEntry> {
        self.name_map
            .get(name)
            .and_then(|index| self.entries.get(*index))
    }

    /// Get a block's default runtime state ID by string ID.
    pub fn default_state_id_by_name(&self, name: &str) -> Option<u32> {
        self.get_by_name(name).map(|block| block.default_state_id)
    }

    /// Iterate over all entries.
    pub fn iter(&self) -> impl Iterator<Item = &BlockEntry> {
        self.entries.iter()
    }

    /// Number of registered entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Return the custom block-property entries for `StartGame`.
    ///
    /// In 1.26.40 this field is the server's custom block-definition list;
    /// vanilla block state palettes are no longer represented by the old
    /// `BlockPropertiesItem` payload. This registry currently contains only
    /// vanilla entries, so emitting an empty list is the exact wire meaning.
    pub fn to_block_properties(&self) -> Vec<jolyne::valentine::ServerBlockProperty> {
        let _ = self;
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vanilla_registry_preserves_block_properties() {
        let mut registry = BlockRegistry::new();
        registry.load_vanilla();
        assert_eq!(registry.len(), unastar_data::blocks::ALL_BLOCKS.len());

        let stone = registry
            .get_by_name("minecraft:stone")
            .expect("stone should be registered");
        assert_eq!(
            stone.default_state_id,
            unastar_data::blocks::get("minecraft:stone")
                .expect("generated stone data exists")
                .default_state_id
        );
        assert_eq!(stone.hardness, 1.5);
        assert_eq!(stone.resistance, 6.0);
        assert!(!stone.is_transparent);
        assert_eq!(stone.filter_light, 15);
    }

    #[test]
    fn state_id_count_uses_canonical_range() {
        let mut registry = BlockRegistry::new();
        registry.load_vanilla();

        let fence_gate = registry
            .get_by_name("minecraft:fence_gate")
            .expect("fence gate should be registered");
        assert_eq!(fence_gate.state_id_count, 16);
        assert_eq!(
            registry
                .get_by_runtime_id(fence_gate.max_state_id)
                .expect("runtime ID should map to fence gate")
                .string_id,
            "minecraft:fence_gate"
        );
    }

    #[test]
    fn duplicate_numeric_ids_do_not_hide_string_or_runtime_lookup() {
        let mut registry = BlockRegistry::new();
        registry.load_vanilla();

        assert_eq!(
            registry
                .get(8)
                .expect("legacy numeric ID 8 should resolve to first generated entry")
                .string_id,
            "minecraft:flowing_water",
            "numeric ID lookup is first-match legacy behavior"
        );

        let grass = registry
            .get_by_name("minecraft:grass_block")
            .expect("grass block should be registered by string ID");
        assert_eq!(grass.id, 8);
        assert_eq!(
            registry
                .get_by_runtime_id(grass.default_state_id)
                .expect("grass default runtime ID should map back to grass")
                .string_id,
            "minecraft:grass_block"
        );
    }

    #[test]
    fn shared_vanilla_registry_resolves_default_state_ids() {
        let registry = BlockRegistry::vanilla();

        assert_eq!(
            registry.default_state_id_by_name("minecraft:stone"),
            registry
                .get_by_name("minecraft:stone")
                .map(|block| block.default_state_id)
        );
        assert!(
            registry
                .default_state_id_by_name("minecraft:not_a_real_block")
                .is_none()
        );
    }
}
