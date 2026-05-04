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
    /// Numeric block ID.
    pub id: u32,
    /// String identifier (e.g., "minecraft:stone").
    pub string_id: String,
    /// Display name.
    pub name: String,
    /// Number of generated typed state variants.
    ///
    /// Some generated Valentine block definitions currently under-report this
    /// for shared state families. Use `state_id_count` for canonical runtime
    /// palette coverage.
    pub state_count: u32,
    /// Number of canonical runtime state IDs in this block's range.
    pub state_id_count: u32,
    /// Minimum runtime state ID (from canonical block states).
    pub min_state_id: u32,
    /// Maximum runtime state ID (from canonical block states).
    pub max_state_id: u32,
    /// Default state ID for this block.
    pub default_state_id: u32,
    /// Block hardness from generated Bedrock data.
    pub hardness: f32,
    /// Explosion resistance from generated Bedrock data.
    pub resistance: f32,
    /// Whether this block is transparent for lighting/render semantics.
    pub is_transparent: bool,
    /// Light emitted by this block.
    pub emit_light: u8,
    /// Light filtered by this block.
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

    /// Load vanilla blocks from valentine's generated data.
    /// Uses MIN_STATE_ID and MAX_STATE_ID from valentine which are the canonical
    /// runtime IDs that match client expectations.
    pub fn load_vanilla(&mut self) {
        use jolyne::valentine::blocks::BLOCKS;

        self.entries.clear();
        self.id_map.clear();
        self.name_map.clear();

        // Find max runtime_id to size the lookup table.
        let max_rid = BLOCKS.iter().map(|b| b.max_state_id()).max().unwrap_or(0);
        self.runtime_id_map = vec![MISSING_INDEX; (max_rid + 1) as usize];

        for block in BLOCKS.iter() {
            let entry = BlockEntry {
                id: block.id(),
                string_id: block.string_id().to_string(),
                name: block.name().to_string(),
                state_count: block.state_count(),
                state_id_count: block.max_state_id() - block.min_state_id() + 1,
                min_state_id: block.min_state_id(),
                max_state_id: block.max_state_id(),
                default_state_id: block.default_state_id(),
                hardness: block.hardness(),
                resistance: block.resistance(),
                is_transparent: block.is_transparent(),
                emit_light: block.emit_light(),
                filter_light: block.filter_light(),
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

    /// Generate BlockPropertyData for PacketStartGame using canonical block palette.
    ///
    /// Each entry is one canonical block state (name + full NBT compound).
    /// The index in this list = block runtime ID, matching canonical_block_states.nbt.
    pub fn to_block_properties(&self) -> Vec<jolyne::valentine::BlockPropertiesItem> {
        use jolyne::valentine::BlockPropertiesItem;
        use jolyne::valentine::block_palette::BLOCK_PALETTE_NBT;
        use valentine::bedrock::codec::Nbt;

        let mut properties = Vec::new();
        let data = BLOCK_PALETTE_NBT;
        let mut pos = 0usize;

        // Parse each Network LE NBT compound from the blob to extract
        // the name and use the raw bytes as the Nbt state
        while pos < data.len() {
            let start = pos;

            // TAG_Compound (0x0a)
            if data[pos] != 0x0a {
                break;
            }
            pos += 1;

            // Root name (VarInt length + bytes) — should be empty (length 0)
            let (root_name_len, consumed) = read_varint(&data[pos..]);
            pos += consumed;
            pos += root_name_len as usize;

            // Scan compound children to extract the "name" field and find the end
            let mut block_name = String::new();
            pos = scan_compound_for_name(data, pos, &mut block_name);

            let raw = &data[start..pos];

            properties.push(BlockPropertiesItem {
                name: block_name,
                state: Nbt(bytes::Bytes::copy_from_slice(raw)),
            });
        }

        properties
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vanilla_registry_preserves_block_properties() {
        let mut registry = BlockRegistry::new();
        registry.load_vanilla();

        let stone = registry
            .get_by_name("minecraft:stone")
            .expect("stone should be registered");
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

/// Read a VarInt from a byte slice, returning (value, bytes_consumed).
fn read_varint(data: &[u8]) -> (u32, usize) {
    let mut result = 0u32;
    let mut shift = 0;
    let mut pos = 0;
    loop {
        if pos >= data.len() {
            break;
        }
        let byte = data[pos];
        pos += 1;
        result |= ((byte & 0x7F) as u32) << shift;
        if byte & 0x80 == 0 {
            break;
        }
        shift += 7;
    }
    (result, pos)
}

/// Scan a compound tag's children, extracting the "name" string field,
/// and return the position after the compound's TAG_End.
fn scan_compound_for_name(data: &[u8], mut pos: usize, name_out: &mut String) -> usize {
    loop {
        if pos >= data.len() {
            break;
        }
        let tag_id = data[pos];
        pos += 1;
        if tag_id == 0 {
            // TAG_End
            break;
        }

        // Read field name
        let (name_len, consumed) = read_varint(&data[pos..]);
        pos += consumed;
        let field_name = std::str::from_utf8(&data[pos..pos + name_len as usize]).unwrap_or("");
        pos += name_len as usize;

        // If this is the "name" field (TAG_String = 8), extract the value
        if tag_id == 8 && field_name == "name" {
            let (str_len, consumed) = read_varint(&data[pos..]);
            pos += consumed;
            *name_out = String::from_utf8_lossy(&data[pos..pos + str_len as usize]).into_owned();
            pos += str_len as usize;
        } else {
            pos = skip_nbt_payload(data, pos, tag_id);
        }
    }
    pos
}

/// Skip an NBT payload based on tag type, returning the new position.
fn skip_nbt_payload(data: &[u8], mut pos: usize, tag_id: u8) -> usize {
    match tag_id {
        1 => pos + 1, // Byte
        2 => pos + 2, // Short
        3 => {
            // Int (ZigZag32 = VarInt)
            let (_, consumed) = read_varint(&data[pos..]);
            pos + consumed
        }
        4 => {
            // Long (ZigZag64 = VarLong)
            let mut shift = 0;
            loop {
                if pos >= data.len() {
                    break;
                }
                let byte = data[pos];
                pos += 1;
                if byte & 0x80 == 0 || shift >= 63 {
                    break;
                }
                shift += 7;
            }
            pos
        }
        5 => pos + 4, // Float
        6 => pos + 8, // Double
        7 => {
            // Byte Array
            let (len, consumed) = read_varint(&data[pos..]);
            pos + consumed + len as usize
        }
        8 => {
            // String
            let (len, consumed) = read_varint(&data[pos..]);
            pos + consumed + len as usize
        }
        9 => {
            // List
            let inner_id = data[pos];
            pos += 1;
            let (count, consumed) = read_varint(&data[pos..]);
            pos += consumed;
            for _ in 0..count {
                pos = skip_nbt_payload(data, pos, inner_id);
            }
            pos
        }
        10 => {
            // Compound
            loop {
                if pos >= data.len() {
                    break;
                }
                let inner_tag = data[pos];
                pos += 1;
                if inner_tag == 0 {
                    break;
                }
                // Skip name
                let (name_len, consumed) = read_varint(&data[pos..]);
                pos += consumed + name_len as usize;
                pos = skip_nbt_payload(data, pos, inner_tag);
            }
            pos
        }
        11 => {
            // Int Array
            let (len, consumed) = read_varint(&data[pos..]);
            pos += consumed;
            for _ in 0..len {
                let (_, c) = read_varint(&data[pos..]);
                pos += c;
            }
            pos
        }
        12 => {
            // Long Array
            let (len, consumed) = read_varint(&data[pos..]);
            pos += consumed;
            for _ in 0..len {
                let mut shift = 0;
                loop {
                    if pos >= data.len() {
                        break;
                    }
                    let byte = data[pos];
                    pos += 1;
                    if byte & 0x80 == 0 || shift >= 63 {
                        break;
                    }
                    shift += 7;
                }
            }
            pos
        }
        _ => pos, // Unknown tag, can't skip
    }
}
