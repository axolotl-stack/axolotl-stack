//! Block registry for runtime block management.
//!
//! Blocks are more complex than items/entities because of block states.
//! Each block type has multiple runtime IDs (one per state combination).

use super::{Registry, RegistryEntry};

/// Runtime block entry in the registry.
#[derive(Debug, Clone)]
pub struct BlockEntry {
    /// Numeric block ID.
    pub id: u32,
    /// String identifier (e.g., "minecraft:stone").
    pub string_id: String,
    /// Display name.
    pub name: String,
    /// Number of states this block has.
    pub state_count: u32,
    /// Minimum runtime state ID (from valentine's canonical order).
    pub min_state_id: u32,
    /// Maximum runtime state ID (from valentine's canonical order).
    pub max_state_id: u32,
    /// Default state ID for this block.
    pub default_state_id: u32,
}

impl RegistryEntry for BlockEntry {
    fn id(&self) -> u32 {
        self.id
    }

    fn string_id(&self) -> &str {
        &self.string_id
    }
}

/// Block registry type alias.
pub type BlockRegistry = Registry<BlockEntry>;

impl BlockRegistry {
    /// Load vanilla blocks from valentine's generated data.
    /// Uses MIN_STATE_ID and MAX_STATE_ID from valentine which are the canonical
    /// runtime IDs that match client expectations.
    pub fn load_vanilla(&mut self) {
        use jolyne::valentine::blocks::BLOCKS;

        for block in BLOCKS.iter() {
            let entry = BlockEntry {
                id: block.id(),
                string_id: block.string_id().to_string(),
                name: block.name().to_string(),
                state_count: block.state_count(),
                min_state_id: block.min_state_id(),
                max_state_id: block.max_state_id(),
                default_state_id: block.default_state_id(),
            };
            let _ = self.register(entry);
        }
    }

    /// Get block entry by runtime ID (state ID).
    pub fn get_by_runtime_id(&self, runtime_id: u32) -> Option<&BlockEntry> {
        // Iterate through all blocks to find the range containing this runtime_id.
        // Optimization: We could store a secondary map or interval tree, but linear scan is okay for now (~1000 blocks).
        self.iter().find(|entry| {
            runtime_id >= entry.min_state_id && runtime_id <= entry.max_state_id
        })
    }

    /// Generate BlockPropertyData for PacketStartGame.
    pub fn to_block_properties(&self) -> Vec<jolyne::valentine::BlockPropertiesItem> {
        use jolyne::valentine::BlockPropertiesItem;
        use jolyne::valentine::blocks::BLOCKS;
        use valentine::bedrock::codec::Nbt;

        let mut properties = Vec::with_capacity(BLOCKS.len());

        for block in BLOCKS.iter() {
            // Since we can't easily get the NBT from BlockDefDyn,
            // we construct a default state definition with empty properties.
            // Bedrock clients usually accept this as the default state of the block.

            // The NBT from block definition is the "block" compound (NbtMap).
            // PacketStartGame expects a list of BlockPropertiesItem, which contains:
            // - name: String (e.g. "minecraft:stone")
            // - state: Nbt (the block state definition)

            properties.push(BlockPropertiesItem {
                name: block.string_id().to_string(),
                state: Nbt::default(),
            });
        }
        properties
    }
}
