//! Block registry for runtime block management.
//!
//! Blocks are more complex than items/entities because of block states.
//! Each block type has multiple runtime IDs (one per state combination).

use super::RegistryEntry;
use std::collections::HashMap;
use std::sync::LazyLock;
pub use unastar_data::blocks::StateValue;

const MISSING_INDEX: usize = usize::MAX;

static VANILLA_BLOCK_REGISTRY: LazyLock<BlockRegistry> = LazyLock::new(|| {
    let mut registry = BlockRegistry::new();
    registry.load_vanilla();
    registry
});

/// Native liquid material identity, resolved once when the registry loads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiquidKind {
    Water,
    Lava,
}

/// Cached per-state facts used by actor liquid motion without string lookups.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiquidState {
    pub kind: LiquidKind,
    /// Native `liquid_depth`, including the falling states 8 through 15.
    pub depth: u8,
}

/// Runtime block entry in the registry.
#[derive(Debug, Clone)]
pub struct BlockEntry {
    /// Registry ID: the block's first canonical runtime state ID.
    pub id: u32,
    /// String identifier (e.g., "minecraft:stone").
    pub string_id: String,
    /// Native identifier, retained in the name field for registry consumers.
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
    /// Native destroy speed of the first canonical state.
    pub hardness: f32,
    /// Native explosion resistance of the first canonical state.
    pub resistance: f32,
    /// Inverse native opaque-full-block flag of the first canonical state.
    pub is_transparent: bool,
    /// Native emitted light level of the first canonical state.
    pub emit_light: u8,
    /// Native light damping of the first canonical state.
    pub filter_light: u8,
    /// Generated native row: tags, physics flags, friction, shape and the
    /// per-state property table.
    pub data: &'static unastar_data::blocks::BlockData,
}

impl BlockEntry {
    /// Whether the native block tag list contains `tag`.
    pub fn has_tag(&self, tag: &str) -> bool {
        self.data.tags.contains(&tag)
    }

    /// Value of a state property in this block's canonical state `runtime_id`.
    pub fn state_value(&self, runtime_id: u32, property: &str) -> Option<StateValue> {
        self.data.state_value(runtime_id, property)
    }

    /// The canonical state equal to `runtime_id` except for `property`.
    pub fn with_state_value(
        &self,
        runtime_id: u32,
        property: &str,
        value: StateValue,
    ) -> Option<u32> {
        self.data.with_state_value(runtime_id, property, value)
    }
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
    /// Maps each block's first canonical state ID to its entry.
    id_map: HashMap<u32, usize>,
    /// Maps string ID to entry index.
    name_map: HashMap<String, usize>,
    /// Maps runtime state ID to entry index. Index = runtime_id.
    runtime_id_map: Vec<usize>,
    /// Water/lava identity and depth, indexed by canonical runtime state.
    liquid_states: Vec<Option<LiquidState>>,
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
    /// Canonical ranges and physical measurements come from protocolgen's
    /// native BDS 1.26.51 snapshot through `unastar-data`.
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
        self.liquid_states = vec![None; self.runtime_id_map.len()];
        let material = |identifier| {
            ALL_BLOCKS
                .iter()
                .find(|block| block.identifier == identifier)
                .expect("native liquid archetype")
                .material
        };
        let water_material = material("minecraft:water");
        let lava_material = material("minecraft:lava");
        assert_ne!(water_material, lava_material, "distinct native liquids");

        for block in ALL_BLOCKS.iter() {
            let entry = BlockEntry {
                id: block.id,
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
                data: block,
            };

            let entry_index = self.entries.len();

            // Fill the runtime_id -> entry mapping for every state.
            for rid in entry.min_state_id..=entry.max_state_id {
                self.runtime_id_map[rid as usize] = entry_index;
            }

            let kind = if block.liquid && block.material == water_material {
                Some(LiquidKind::Water)
            } else if block.liquid && block.material == lava_material {
                Some(LiquidKind::Lava)
            } else {
                None
            };
            if let Some(kind) = kind {
                let column = block
                    .properties
                    .iter()
                    .position(|property| property.name == "liquid_depth")
                    .expect("native water/lava depth property");
                let property = &block.properties[column];
                for rid in block.min_state_id..=block.max_state_id {
                    let row = (rid - block.min_state_id) as usize * block.properties.len();
                    let index = usize::from(block.state_values[row + column]);
                    let StateValue::Int(value) = property.values[index] else {
                        panic!("native liquid depth must be an integer");
                    };
                    let depth = u8::try_from(value)
                        .ok()
                        .filter(|depth| *depth <= 15)
                        .expect("native liquid depth must be in 0..=15");
                    self.liquid_states[rid as usize] = Some(LiquidState { kind, depth });
                }
            }

            // Registry IDs are unique first-palette ordinals.
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

    /// Water/lava state facts; unknown or non-liquid states return `None`.
    #[inline]
    pub fn liquid_state(&self, runtime_id: u32) -> Option<LiquidState> {
        self.liquid_states
            .get(runtime_id as usize)
            .copied()
            .flatten()
    }

    /// Get entry by registry ID (the first canonical state ID). O(1).
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
    fn registry_ids_match_first_canonical_state() {
        let registry = BlockRegistry::vanilla();
        let stone = registry.get(3317).expect("native stone registry ID");
        assert_eq!(stone.string_id, "minecraft:stone");
        let grass = registry.get_by_name("minecraft:grass_block").unwrap();
        assert_eq!(grass.id, grass.min_state_id);
        assert_eq!(
            registry
                .get_by_runtime_id(grass.default_state_id)
                .unwrap()
                .string_id,
            grass.string_id
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
