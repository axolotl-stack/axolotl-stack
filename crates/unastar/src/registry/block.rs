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
    /// Minimum runtime state ID (from canonical block states).
    pub min_state_id: u32,
    /// Maximum runtime state ID (from canonical block states).
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

/// Block registry with O(1) runtime_id → block lookup.
#[derive(Debug, Clone, Default)]
pub struct BlockRegistry {
    inner: Registry<BlockEntry>,
    /// Maps runtime state ID → block entry ID. Index = runtime_id, value = block id.
    runtime_id_map: Vec<u32>,
}

impl BlockRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Load vanilla blocks from valentine's generated data.
    /// Uses MIN_STATE_ID and MAX_STATE_ID from valentine which are the canonical
    /// runtime IDs that match client expectations.
    pub fn load_vanilla(&mut self) {
        use jolyne::valentine::blocks::BLOCKS;

        // Find max runtime_id to size the lookup table
        let max_rid = BLOCKS.iter().map(|b| b.max_state_id()).max().unwrap_or(0);

        self.runtime_id_map = vec![u32::MAX; (max_rid + 1) as usize];

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

            // Fill the runtime_id → block_id mapping for every state
            for rid in entry.min_state_id..=entry.max_state_id {
                self.runtime_id_map[rid as usize] = entry.id;
            }

            let _ = self.inner.register(entry);
        }
    }

    /// Get block entry by runtime ID (state ID). O(1).
    #[inline]
    pub fn get_by_runtime_id(&self, runtime_id: u32) -> Option<&BlockEntry> {
        let block_id = *self.runtime_id_map.get(runtime_id as usize)?;
        if block_id == u32::MAX {
            return None;
        }
        self.inner.get(block_id)
    }

    /// Get entry by block ID. O(1).
    #[inline]
    pub fn get(&self, id: u32) -> Option<&BlockEntry> {
        self.inner.get(id)
    }

    /// Get entry by string ID (linear scan).
    pub fn get_by_name(&self, name: &str) -> Option<&BlockEntry> {
        self.inner.get_by_name(name)
    }

    /// Iterate over all entries.
    pub fn iter(&self) -> impl Iterator<Item = &BlockEntry> {
        self.inner.iter()
    }

    /// Number of registered entries.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
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
