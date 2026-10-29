//! Block registry for runtime block management.
//!
//! Blocks are more complex than items/entities because of block states.
//! Each block type has multiple runtime IDs (one per state combination).

use super::RegistryEntry;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};
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
    /// Hashed network id of each canonical runtime state. Index = runtime_id.
    network_hashes: Vec<u32>,
    /// Canonical runtime state of each hashed network id.
    runtime_by_hash: HashMap<u32, u32>,
    /// Neighbour connection facts and canonical transitions, resolved at load.
    pub(super) connections: Arc<super::collision::ConnectionCache>,
}

/// How block states are numbered on the wire, as `StartGame`'s
/// `block_network_ids_are_hashes` advertises to the client.
///
/// Internally (chunks, storage, items, plugins) a block state is always its
/// canonical palette ordinal; only packet encoding and decoding translate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockNetworkIds {
    /// Each state's 32-bit FNV-1a hash of its `{name, states}` NBT, independent
    /// of palette order. BDS 1.26.51 advertises this.
    #[default]
    Hashed,
    /// The canonical palette ordinal. The client numbers its built-in blocks
    /// plus the blocks `StartGame` defines, so this requires
    /// [`BlockRegistry::to_block_properties`] to define every data-driven block.
    Sequential,
}

impl BlockNetworkIds {
    /// `StartGame.block_network_ids_are_hashes`.
    pub fn is_hashed(self) -> bool {
        self == Self::Hashed
    }

    /// Wire id of a canonical runtime state of the vanilla palette.
    ///
    /// # Panics
    /// If `runtime_id` is not a vanilla canonical state; internal block ids
    /// always are.
    pub fn to_wire(self, runtime_id: u32) -> u32 {
        match self {
            Self::Sequential => runtime_id,
            Self::Hashed => BlockRegistry::vanilla()
                .network_hash(runtime_id)
                .unwrap_or_else(|| {
                    panic!("block runtime id {runtime_id} is not a canonical state")
                }),
        }
    }

    /// Canonical runtime state of a wire id a client sent, if it names one.
    pub fn from_wire(self, wire_id: u32) -> Option<u32> {
        let registry = BlockRegistry::vanilla();
        match self {
            Self::Sequential => registry.get_by_runtime_id(wire_id).map(|_| wire_id),
            Self::Hashed => registry.runtime_by_hash.get(&wire_id).copied(),
        }
    }

    /// [`Self::to_wire`] for an item descriptor's `block_runtime_id`.
    ///
    /// Item stacks store a canonical ordinal where 0 doubles as "not a block".
    /// Ordinal 0 is also a real state (the sole state of one vanilla block,
    /// `minecraft:cyan_terracotta` in 1.26.51), so the item decides: BDS sends
    /// that block item with its hash, every other 0 stays 0.
    pub fn item_block_to_wire(self, item_network_id: i32, runtime_id: u32) -> u32 {
        if runtime_id == 0 && item_network_id != super::item::ordinal_zero_block_item() {
            0
        } else {
            self.to_wire(runtime_id)
        }
    }

    /// [`Self::from_wire`] for an item descriptor's `block_runtime_id`.
    pub fn item_block_from_wire(self, wire_id: u32) -> Option<u32> {
        if wire_id == 0 {
            Some(0)
        } else {
            self.from_wire(wire_id)
        }
    }
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
        self.network_hashes = vec![0; self.runtime_id_map.len()];
        self.runtime_by_hash.clear();
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
                let hash = block.network_hashes[(rid - entry.min_state_id) as usize];
                self.network_hashes[rid as usize] = hash;
                assert!(
                    self.runtime_by_hash.insert(hash, rid).is_none(),
                    "duplicate block network hash {hash}"
                );
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
        self.connections = Arc::new(super::collision::ConnectionCache::new(self));
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

    /// Hashed network id of a canonical runtime state.
    #[inline]
    pub fn network_hash(&self, runtime_id: u32) -> Option<u32> {
        self.get_by_runtime_id(runtime_id)?;
        self.network_hashes.get(runtime_id as usize).copied()
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

    /// `StartGame` block definitions: every vanilla data-driven block (behavior
    /// pack `blocks/`, e.g. concrete and wool slabs/stairs), as BDS sends them.
    ///
    /// With sequential network ids the client numbers its built-in blocks plus
    /// the blocks defined here, sorted by name hash. Runtime ids here are the
    /// full canonical palette, which includes the data-driven blocks, so leaving
    /// any out shifts every later client id and the client renders wrong blocks.
    pub fn to_block_properties(&self) -> Vec<jolyne::valentine::ServerBlockProperty> {
        self.entries
            .iter()
            .filter_map(|entry| {
                let definition = entry.data.server_definition?;
                Some(jolyne::valentine::ServerBlockProperty {
                    block_name: entry.string_id.as_str().into(),
                    block_definition: valentine::bedrock::codec::Nbt(bytes::Bytes::from_static(
                        definition,
                    )),
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ordinal 0 is a real block state, so a hashed client must receive the
    /// block item's hash (as BDS 1.26.51's CraftingData does), while a
    /// block-less item keeps 0 in both numberings.
    #[test]
    fn ordinal_zero_block_item_is_hashed_but_plain_items_stay_zero() {
        let block = BlockRegistry::vanilla().get_by_runtime_id(0).unwrap();
        let items = &*crate::item::test_support::VANILLA;
        let item = items.get_by_name(&block.name).unwrap().network_id();
        let stick = items.get_by_name("minecraft:stick").unwrap().network_id();
        assert_eq!(block.name, "minecraft:cyan_terracotta");
        assert_eq!(
            BlockNetworkIds::Hashed.item_block_to_wire(item, 0),
            973_836_165
        );
        assert_eq!(BlockNetworkIds::Hashed.item_block_to_wire(stick, 0), 0);
        assert_eq!(BlockNetworkIds::Sequential.item_block_to_wire(item, 0), 0);
        assert_eq!(
            BlockNetworkIds::Hashed.item_block_from_wire(973_836_165),
            Some(0)
        );
    }

    /// The client's sequential palette is its built-in blocks plus the blocks
    /// `StartGame` defines. That union must be exactly the canonical palette
    /// behind unastar's runtime ids, or every block after a gap is misnumbered.
    #[test]
    fn start_game_defines_every_vanilla_data_driven_block() {
        let defined: std::collections::BTreeSet<String> = BlockRegistry::vanilla()
            .to_block_properties()
            .iter()
            .map(|property| property.block_name.to_string())
            .collect();
        let pack_dir = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../unastar-data/data/vanilla_bp/behavior_pack/blocks"
        );
        let mut data_driven = std::collections::BTreeSet::new();
        for entry in std::fs::read_dir(pack_dir).expect("vanilla behavior pack blocks") {
            let path = entry.unwrap().path();
            if path
                .extension()
                .is_some_and(|extension| extension == "json")
            {
                let definition: serde_json::Value =
                    serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
                let name = definition["minecraft:block"]["description"]["identifier"]
                    .as_str()
                    .unwrap_or_else(|| panic!("{} has no block identifier", path.display()));
                data_driven.insert(name.to_owned());
            }
        }
        assert_eq!(data_driven.len(), 98);
        assert_eq!(defined, data_driven);
    }

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
