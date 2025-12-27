//! Chunk data structures and encoding - Bedrock protocol format.
//!
//! Uses SubChunkRequestModeLimited (-2) to let clients request sub-chunks on demand.
//! This is the modern approach used by all major Bedrock servers (Dragonfly, etc.).

use bytes::{BufMut, BytesMut};

/// Constants for sub-chunk request modes.
pub mod request_mode {
    /// Client requests sub-chunks on demand via SubChunkRequest packet.
    /// LevelChunk payload contains only biome data + border blocks.
    pub const LIMITED: i32 = -2;

    /// Legacy mode: all sub-chunks sent inline (not recommended).
    #[allow(dead_code)]
    pub const LEGACY: i32 = -1;
}

/// Number of sub-chunks in a full world height (-64 to 320 = 384 blocks = 24 subchunks).
pub const SUBCHUNK_COUNT: u16 = 24;

/// Minimum Y coordinate for the overworld.
pub const MIN_Y: i32 = -64;

/// Blocks per subchunk dimension.
pub const SUBCHUNK_SIZE: usize = 16;

/// Total blocks per subchunk.
pub const BLOCKS_PER_SUBCHUNK: usize = SUBCHUNK_SIZE * SUBCHUNK_SIZE * SUBCHUNK_SIZE;

/// Sub-chunk version for network encoding.
const SUBCHUNK_VERSION: u8 = 9;

/// Block runtime IDs from valentine's canonical block_states.nbt ordering.
/// These values come from the MIN_STATE_ID/default state in valentine blocks.rs.
/// IMPORTANT: These must match client expectations - the client uses
/// block_states.nbt to determine what runtime ID = what block.
pub mod blocks {
    // Values from valentine/bedrock_versions/v1_21_130/src/blocks.rs
    // Air: MIN_STATE_ID = 12530
    pub const AIR: u32 = 12530;
    // Stone: MIN_STATE_ID = 2532
    pub const STONE: u32 = 2532;
    // GrassBlock: MIN_STATE_ID = 11062
    pub const GRASS_BLOCK: u32 = 11062;
    // Dirt: MIN_STATE_ID = 9852
    pub const DIRT: u32 = 9852;
}

/// Height map for a chunk - tracks highest light-blocking block per column.
///
/// This is critical for sub-chunk rendering - the client uses heightmap data
/// to determine lighting and which surfaces to render.
#[derive(Debug, Clone)]
pub struct HeightMap {
    /// Highest light-blocking Y coordinate for each (x, z) column.
    /// Index = (z << 4) | x (256 values total)
    /// Value is the absolute Y coordinate of the first air block above the highest light blocker
    /// (e.g., 16 for a solid column up to Y=15).
    data: [i16; 256],
}

impl HeightMap {
    /// Create a new height map initialized to minimum world Y.
    pub fn new() -> Self {
        Self {
            data: [MIN_Y as i16; 256],
        }
    }

    /// Get the height at a specific column.
    #[inline]
    pub fn at(&self, x: u8, z: u8) -> i16 {
        self.data[((z as usize) << 4) | (x as usize)]
    }

    /// Set the height at a specific column.
    #[inline]
    pub fn set(&mut self, x: u8, z: u8, y: i16) {
        self.data[((z as usize) << 4) | (x as usize)] = y;
    }

    /// Calculate heightmap data for a specific sub-chunk index.
    ///
    /// Returns (type, data) where:
    /// - TooHigh: all columns have their surface above this subchunk
    /// - TooLow: all columns have their surface below this subchunk
    /// - HasData: mixed heights, returns 256 i8 values (cast to u8 for wire format)
    ///
    /// Values in the data array:
    /// - -1 (0xFF as u8): surface is below this subchunk
    /// - 0-15: relative Y within this subchunk
    /// - 16: surface is above this subchunk
    pub fn for_subchunk(&self, sub_y_index: i32) -> (HeightMapType, Option<[u8; 256]>) {
        let subchunk_min_y = sub_y_index * 16;
        let subchunk_max_y = subchunk_min_y + 15;

        let mut all_above = true;
        let mut all_below = true;

        for z in 0u8..16 {
            for x in 0u8..16 {
                let height = self.at(x, z) as i32;

                if height > subchunk_max_y {
                    // Heightmap is above this subchunk.
                    all_below = false;
                } else if height < subchunk_min_y {
                    // Heightmap is below this subchunk.
                    all_above = false;
                } else {
                    // Heightmap is within this subchunk.
                    all_above = false;
                    all_below = false;
                }
            }
        }

        if all_above {
            // All columns have surface above this subchunk
            (HeightMapType::TooHigh, None)
        } else if all_below {
            // All columns have surface below this subchunk
            (HeightMapType::TooLow, None)
        } else {
            // Mixed - surface is somewhere in this subchunk for some columns
            // Now using proper FixedArray encoding
            let mut data = [0u8; 256];
            for z in 0u8..16 {
                for x in 0u8..16 {
                    let idx = ((z as usize) << 4) | (x as usize);
                    let height = self.at(x, z) as i32;

                    let relative = if height < subchunk_min_y {
                        -1i8
                    } else if height > subchunk_max_y {
                        16i8
                    } else {
                        (height - subchunk_min_y) as i8
                    };
                    data[idx] = relative as u8;
                }
            }
            (HeightMapType::HasData, Some(data))
        }
    }
}

impl Default for HeightMap {
    fn default() -> Self {
        Self::new()
    }
}

/// Height map type for sub-chunk responses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeightMapType {
    /// All columns have surface above this subchunk
    TooHigh,
    /// All columns have surface below this subchunk
    TooLow,
    /// Mixed - includes 256 bytes of height data
    HasData,
}

/// A single chunk (16x384x16 blocks) in Bedrock format.
#[derive(Debug, Clone)]
pub struct Chunk {
    pub x: i32,
    pub z: i32,
    /// Sub-chunks (index 0 = Y=-64 to Y=-48, etc.)
    sub_chunks: Vec<SubChunk>,
    /// Biome ID for each sub-chunk section (simplified: one per vertical section)
    biome_ids: Vec<u32>,
    /// Height map tracking highest light-blocking block per column
    height_map: HeightMap,
    /// Whether height map needs recalculation (reserved for future lazy recalculation)
    #[allow(dead_code)]
    recalculate_height_map: bool,
}

/// A single 16x16x16 sub-chunk.
#[derive(Debug, Clone)]
pub struct SubChunk {
    /// Block storage (single layer for now).
    storage: PalettedStorage,
}

/// Block storage using a palette for compression.
#[derive(Debug, Clone)]
pub struct PalettedStorage {
    /// Palette of runtime block IDs (unique blocks in this storage).
    palette: Vec<u32>,
    /// Block indices into the palette (4096 values, one per block).
    /// Only used if palette.len() > 1.
    indices: Vec<u16>,
}

impl Chunk {
    /// Create a new empty chunk at the given coordinates.
    pub fn new(x: i32, z: i32) -> Self {
        let sub_chunks = (0..SUBCHUNK_COUNT).map(|_| SubChunk::empty()).collect();

        let biome_ids = vec![0; SUBCHUNK_COUNT as usize]; // Plains biome

        Self {
            x,
            z,
            sub_chunks,
            biome_ids,
            height_map: HeightMap::new(),
            recalculate_height_map: false,
        }
    }

    /// Fill the bottom layers with grass blocks.
    ///
    /// # Arguments
    /// * `layers` - Number of Y layers to fill (from world Y=0 upward).
    pub fn fill_floor(&mut self, layers: u32, block_id: u32) {
        // Y=0 is at subchunk index 4 (since MIN_Y=-64, so subchunk 0 covers Y=-64 to -48)
        // Subchunk 4 covers Y=0 to Y=15
        let base_subchunk_idx = 4usize;

        // Fill the specified layers
        for layer_y in 0..layers {
            let world_y = layer_y as i32;
            let subchunk_idx = base_subchunk_idx + (world_y / 16) as usize;
            let local_y = (world_y % 16) as usize;

            if subchunk_idx < self.sub_chunks.len() {
                self.sub_chunks[subchunk_idx].fill_layer(local_y, block_id);
            }
        }
    }

    /// Fill an entire subchunk (16x16x16) with a single block type.
    /// Uses single-value palette for efficient encoding.
    ///
    /// # Arguments
    /// * `subchunk_idx` - Array index of subchunk to fill (0-23)
    /// * `block_id` - Runtime block ID to fill with
    pub fn fill_subchunk_solid(&mut self, subchunk_idx: usize, block_id: u32) {
        if subchunk_idx < self.sub_chunks.len() {
            self.sub_chunks[subchunk_idx].fill_solid(block_id);

            // Update height map if this is a non-air block
            if block_id != blocks::AIR {
                // Calculate the world Y for the top of this subchunk
                let subchunk_top_y = (MIN_Y + (subchunk_idx as i32 + 1) * 16) as i16;

                // Update all columns to have height at top of this subchunk
                for z in 0u8..16 {
                    for x in 0u8..16 {
                        let current = self.height_map.at(x, z);
                        if subchunk_top_y > current {
                            self.height_map.set(x, z, subchunk_top_y);
                        }
                    }
                }
            }
        }
    }

    /// Get the index of the highest non-empty sub-chunk.
    pub fn highest_subchunk(&self) -> u16 {
        for (i, sub) in self.sub_chunks.iter().enumerate().rev() {
            if !sub.is_empty() {
                return i as u16;
            }
        }
        0
    }

    /// Encode biome data only (for SubChunkRequestModeLimited).
    /// This is the payload format when sub_chunk_count = -2.
    pub fn encode_biomes(&self) -> Vec<u8> {
        let mut buf = BytesMut::with_capacity(SUBCHUNK_COUNT as usize * 2 + 1);

        // Encode biome storage for each vertical section
        for &biome_id in &self.biome_ids {
            encode_biome_section(&mut buf, biome_id);
        }

        // Border blocks count (0 = none)
        buf.put_u8(0);

        buf.to_vec()
    }

    /// Encode a specific sub-chunk's data for SubChunk response.
    /// Returns None if the sub-chunk doesn't exist.
    pub fn encode_subchunk(&self, y_index: i32) -> Option<Vec<u8>> {
        // Convert absolute Y index to array index
        let array_idx = (y_index - (MIN_Y >> 4)) as usize;
        let sub = self.sub_chunks.get(array_idx)?;

        let mut buf = BytesMut::with_capacity(1024);
        sub.encode(&mut buf, y_index as i8);
        Some(buf.to_vec())
    }

    /// Check if sub-chunk at given Y index is all air.
    pub fn is_subchunk_empty(&self, y_index: i32) -> bool {
        let array_idx = (y_index - (MIN_Y >> 4)) as usize;
        self.sub_chunks
            .get(array_idx)
            .map(|s| s.is_empty())
            .unwrap_or(true)
    }

    /// Get the height map for this chunk.
    pub fn height_map(&self) -> &HeightMap {
        &self.height_map
    }

    /// Get heightmap data for a specific subchunk Y index.
    ///
    /// This returns the data in the format expected by SubChunk responses.
    pub fn get_subchunk_heightmap(&self, sub_y_index: i32) -> (HeightMapType, Option<[u8; 256]>) {
        self.height_map.for_subchunk(sub_y_index)
    }

    /// Get the block at world coordinates.
    ///
    /// # Arguments
    /// * `x` - Local X coordinate (0-15)
    /// * `y` - World Y coordinate (-64 to 319)
    /// * `z` - Local Z coordinate (0-15)
    ///
    /// Returns the block runtime ID, or AIR if out of bounds.
    pub fn get_block(&self, x: u8, y: i16, z: u8) -> u32 {
        let adjusted_y = (y as i32) - MIN_Y;
        if adjusted_y < 0 || adjusted_y >= (SUBCHUNK_COUNT as i32 * 16) {
            return blocks::AIR;
        }

        let subchunk_idx = (adjusted_y / 16) as usize;
        let local_y = (adjusted_y % 16) as u8;

        self.sub_chunks
            .get(subchunk_idx)
            .map(|s| s.get_block(x, local_y, z))
            .unwrap_or(blocks::AIR)
    }

    /// Set the block at world coordinates.
    ///
    /// # Arguments
    /// * `x` - Local X coordinate (0-15)
    /// * `y` - World Y coordinate (-64 to 319)
    /// * `z` - Local Z coordinate (0-15)
    /// * `block_id` - Block runtime ID to set
    ///
    /// Returns the previous block runtime ID, or None if out of bounds.
    pub fn set_block(&mut self, x: u8, y: i16, z: u8, block_id: u32) -> Option<u32> {
        let adjusted_y = (y as i32) - MIN_Y;
        if adjusted_y < 0 || adjusted_y >= (SUBCHUNK_COUNT as i32 * 16) {
            return None;
        }

        let subchunk_idx = (adjusted_y / 16) as usize;
        let local_y = (adjusted_y % 16) as u8;

        let old = self.sub_chunks[subchunk_idx].set_block(x, local_y, z, block_id);

        // Update height map if needed
        self.update_heightmap_for_block(x, y, z, block_id);

        Some(old)
    }

    /// Update the height map after a block change.
    fn update_heightmap_for_block(&mut self, x: u8, y: i16, z: u8, block_id: u32) {
        let current_height = self.height_map.at(x, z);

        if block_id != blocks::AIR {
            // Non-air block: update height if higher than current
            if y >= current_height {
                self.height_map.set(x, z, y + 1);
            }
        } else if y + 1 == current_height {
            // Air block at the current height - need to recalculate
            // Scan downward to find new highest non-air block
            let mut new_height = MIN_Y as i16;
            for check_y in (MIN_Y as i16..y).rev() {
                if self.get_block(x, check_y, z) != blocks::AIR {
                    new_height = check_y + 1;
                    break;
                }
            }
            self.height_map.set(x, z, new_height);
        }
    }

    /// Decode a subchunk from disk/network format.
    ///
    /// # Arguments
    /// * `y_index` - The absolute Y index of the subchunk (e.g., -4 to 19 for overworld)
    /// * `data` - The encoded subchunk bytes
    ///
    /// Returns an error string if decoding failed.
    pub fn decode_subchunk(&mut self, y_index: i32, data: &[u8]) -> Result<(), String> {
        // Convert absolute Y index to array index
        let array_idx = (y_index - (MIN_Y >> 4)) as usize;
        if array_idx >= self.sub_chunks.len() {
            return Err(format!("Y index {} out of range", y_index));
        }

        if data.is_empty() {
            return Err("Empty subchunk data".to_string());
        }

        let version = data[0];
        if version != SUBCHUNK_VERSION && version != 8 {
            // Accept version 8 as well (similar format)
            return Err(format!("Unsupported subchunk version: {}", version));
        }

        if data.len() < 3 {
            return Err("Subchunk data too short".to_string());
        }

        let storage_count = data[1];
        // Skip y_index byte (data[2]) - we already know the Y from the key

        let mut offset = 3;

        // Decode each storage layer (usually just 1)
        for _ in 0..storage_count {
            if offset >= data.len() {
                break;
            }

            let storage = PalettedStorage::decode(&data[offset..])?;
            offset += storage.1; // Skip bytes consumed

            // Replace the subchunk's storage with the decoded one
            self.sub_chunks[array_idx].storage = storage.0;

            // Only use first storage layer
            break;
        }

        // Update heightmap for this subchunk
        let base_y = y_index * 16;
        for x in 0u8..16 {
            for z in 0u8..16 {
                for local_y in (0..16).rev() {
                    let block = self.sub_chunks[array_idx].get_block(x, local_y, z);
                    if block != blocks::AIR {
                        let world_y = (base_y + local_y as i32) as i16;
                        let current = self.height_map.at(x, z);
                        if world_y + 1 > current {
                            self.height_map.set(x, z, world_y + 1);
                        }
                        break;
                    }
                }
            }
        }

        Ok(())
    }
}

impl Default for Chunk {
    fn default() -> Self {
        Self::new(0, 0)
    }
}

impl SubChunk {
    /// Create an empty (all air) sub-chunk.
    pub fn empty() -> Self {
        Self {
            storage: PalettedStorage::single_block(blocks::AIR),
        }
    }

    /// Check if this subchunk is all air.
    pub fn is_empty(&self) -> bool {
        self.storage.palette.len() == 1 && self.storage.palette[0] == blocks::AIR
    }

    /// Fill an entire Y layer with a block.
    pub fn fill_layer(&mut self, y: usize, block_id: u32) {
        self.storage.fill_layer(y, block_id);
    }

    /// Fill the entire subchunk with a single block type.
    /// Uses single-value palette for maximum efficiency.
    pub fn fill_solid(&mut self, block_id: u32) {
        self.storage = PalettedStorage::single_block(block_id);
    }

    /// Get the block at local coordinates (0-15 for each axis).
    #[inline]
    pub fn get_block(&self, x: u8, y: u8, z: u8) -> u32 {
        self.storage.get_block(x as usize, y as usize, z as usize)
    }

    /// Set the block at local coordinates (0-15 for each axis).
    /// Returns the previous block runtime ID.
    #[inline]
    pub fn set_block(&mut self, x: u8, y: u8, z: u8, block_id: u32) -> u32 {
        self.storage
            .set_block(x as usize, y as usize, z as usize, block_id)
    }

    /// Encode this sub-chunk for network transmission.
    fn encode(&self, buf: &mut BytesMut, y_index: i8) {
        // SubChunk format v9:
        // - Version: u8 = 9
        // - Storage count: u8
        // - Y index: i8 (relative to world min Y)
        // - For each storage: paletted data

        buf.put_u8(SUBCHUNK_VERSION);
        buf.put_u8(1); // Single storage layer
        buf.put_i8(y_index);

        self.storage.encode(buf);
    }
}

impl PalettedStorage {
    /// Create storage with a single block type (most efficient).
    fn single_block(runtime_id: u32) -> Self {
        Self {
            palette: vec![runtime_id],
            indices: vec![],
        }
    }

    /// Fill an entire Y layer with a block.
    fn fill_layer(&mut self, y: usize, block_id: u32) {
        // Ensure we have the block in palette
        let palette_idx = self.get_or_add_palette_entry(block_id);

        // Initialize indices if needed
        if self.indices.is_empty() && self.palette.len() > 1 {
            // Was single block, now has multiple - fill with index 0
            self.indices = vec![0; BLOCKS_PER_SUBCHUNK];
        }

        // Fill the layer
        // Bedrock uses XZY index order: index = (x << 8) | (z << 4) | y
        if !self.indices.is_empty() {
            for x in 0..SUBCHUNK_SIZE {
                for z in 0..SUBCHUNK_SIZE {
                    // XZY order: index = (x * 256) + (z * 16) + y
                    let idx = (x << 8) | (z << 4) | y;
                    self.indices[idx] = palette_idx;
                }
            }
        }
    }

    /// Get block runtime ID at the given local coordinates (0-15 each).
    #[inline]
    fn get_block(&self, x: usize, y: usize, z: usize) -> u32 {
        if self.palette.len() == 1 {
            // Single-value palette
            self.palette[0]
        } else {
            // XZY index order: (x << 8) | (z << 4) | y
            let idx = (x << 8) | (z << 4) | y;
            let palette_idx = self.indices.get(idx).copied().unwrap_or(0) as usize;
            self.palette
                .get(palette_idx)
                .copied()
                .unwrap_or(blocks::AIR)
        }
    }

    /// Set block runtime ID at the given local coordinates (0-15 each).
    /// Returns the previous runtime ID.
    fn set_block(&mut self, x: usize, y: usize, z: usize, block_id: u32) -> u32 {
        let old = self.get_block(x, y, z);
        if old == block_id {
            return old;
        }

        let palette_idx = self.get_or_add_palette_entry(block_id);

        // Initialize indices if needed (transitioning from single-value palette)
        if self.indices.is_empty() && self.palette.len() > 1 {
            self.indices = vec![0; BLOCKS_PER_SUBCHUNK];
        }

        if !self.indices.is_empty() {
            // XZY index order: (x << 8) | (z << 4) | y
            let idx = (x << 8) | (z << 4) | y;
            self.indices[idx] = palette_idx;
        }

        old
    }

    /// Get palette index for a block, adding it if needed.
    fn get_or_add_palette_entry(&mut self, block_id: u32) -> u16 {
        for (i, &id) in self.palette.iter().enumerate() {
            if id == block_id {
                return i as u16;
            }
        }
        let idx = self.palette.len() as u16;
        self.palette.push(block_id);
        idx
    }

    /// Calculate bits needed per block index.
    fn bits_per_block(&self) -> u8 {
        if self.palette.len() <= 1 {
            0
        } else if self.palette.len() <= 2 {
            1
        } else if self.palette.len() <= 4 {
            2
        } else if self.palette.len() <= 8 {
            3
        } else if self.palette.len() <= 16 {
            4
        } else if self.palette.len() <= 32 {
            5
        } else if self.palette.len() <= 64 {
            6
        } else if self.palette.len() <= 128 {
            7
        } else if self.palette.len() <= 256 {
            8
        } else {
            16
        }
    }

    /// Encode this storage for network transmission.
    fn encode(&self, buf: &mut BytesMut) {
        let bits = self.bits_per_block();

        // Network encoding header: (bits_per_block << 1) | 1
        let header = (bits << 1) | 1;
        buf.put_u8(header);

        if bits == 0 {
            // Single-value: just the palette entry as signed varint
            write_signed_varint32(buf, self.palette[0] as i32);
        } else {
            // Multi-value: word-aligned indices followed by palette

            // Calculate how many indices fit per 32-bit word
            let indices_per_word = 32 / bits as usize;
            let word_count = (BLOCKS_PER_SUBCHUNK + indices_per_word - 1) / indices_per_word;

            // Encode indices as packed u32 words
            for word_idx in 0..word_count {
                let mut word: u32 = 0;
                for i in 0..indices_per_word {
                    let block_idx = word_idx * indices_per_word + i;
                    if block_idx < BLOCKS_PER_SUBCHUNK {
                        let palette_idx = self.indices.get(block_idx).copied().unwrap_or(0) as u32;
                        word |= (palette_idx & ((1 << bits) - 1)) << (i * bits as usize);
                    }
                }
                buf.put_u32_le(word);
            }

            // Palette length as signed varint
            write_signed_varint32(buf, self.palette.len() as i32);

            // Palette entries as signed varints
            for &runtime_id in &self.palette {
                write_signed_varint32(buf, runtime_id as i32);
            }
        }
    }

    /// Decode a paletted storage from bytes.
    /// Returns (storage, bytes_consumed).
    fn decode(data: &[u8]) -> Result<(Self, usize), String> {
        if data.is_empty() {
            return Err("Empty storage data".to_string());
        }

        let header = data[0];
        let bits = header >> 1;
        let mut offset = 1;

        if bits == 0 {
            // Single-value palette
            let (value, consumed) =
                read_signed_varint32(&data[offset..]).ok_or("Failed to read palette value")?;
            offset += consumed;

            Ok((
                Self {
                    palette: vec![value as u32],
                    indices: vec![],
                },
                offset,
            ))
        } else {
            // Multi-value palette
            let indices_per_word = 32 / bits as usize;
            let word_count = (BLOCKS_PER_SUBCHUNK + indices_per_word - 1) / indices_per_word;
            let word_bytes = word_count * 4;

            if data.len() < offset + word_bytes {
                return Err("Not enough data for indices".to_string());
            }

            // Read packed indices
            let mut indices = vec![0u16; BLOCKS_PER_SUBCHUNK];
            let mask = (1u32 << bits) - 1;

            for word_idx in 0..word_count {
                let word_offset = offset + word_idx * 4;
                let word = u32::from_le_bytes([
                    data[word_offset],
                    data[word_offset + 1],
                    data[word_offset + 2],
                    data[word_offset + 3],
                ]);

                for i in 0..indices_per_word {
                    let block_idx = word_idx * indices_per_word + i;
                    if block_idx < BLOCKS_PER_SUBCHUNK {
                        let palette_idx = (word >> (i * bits as usize)) & mask;
                        indices[block_idx] = palette_idx as u16;
                    }
                }
            }
            offset += word_bytes;

            // Read palette length
            let (palette_len, consumed) =
                read_signed_varint32(&data[offset..]).ok_or("Failed to read palette length")?;
            offset += consumed;

            // Read palette entries
            let mut palette = Vec::with_capacity(palette_len as usize);
            for _ in 0..palette_len {
                let (value, consumed) =
                    read_signed_varint32(&data[offset..]).ok_or("Failed to read palette entry")?;
                offset += consumed;
                palette.push(value as u32);
            }

            Ok((Self { palette, indices }, offset))
        }
    }
}

/// Encode a single biome section (16x16x16 area).
fn encode_biome_section(buf: &mut BytesMut, biome_id: u32) {
    // Network biome palettes use signed VarInt32 (zigzag) entries, just like blocks.
    // Single-value palette (bits_per_block = 0): header = 1, then a single palette value.
    buf.put_u8(1); // Header: (0 << 1) | 1 (network)
    write_signed_varint32(buf, biome_id as i32);
}

/// Write a signed varint32 (zigzag encoded).
fn write_signed_varint32(buf: &mut BytesMut, value: i32) {
    let zigzag = ((value << 1) ^ (value >> 31)) as u32;
    write_unsigned_varint32(buf, zigzag);
}

/// Write an unsigned varint32.
fn write_unsigned_varint32(buf: &mut BytesMut, mut value: u32) {
    loop {
        let mut byte = (value & 0x7F) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        buf.put_u8(byte);
        if value == 0 {
            break;
        }
    }
}

/// Read a signed varint32 (zigzag encoded).
/// Returns (value, bytes_consumed) or None if invalid.
fn read_signed_varint32(data: &[u8]) -> Option<(i32, usize)> {
    let (zigzag, consumed) = read_unsigned_varint32(data)?;
    // Decode zigzag: (n >> 1) ^ -(n & 1)
    let value = ((zigzag >> 1) as i32) ^ -((zigzag & 1) as i32);
    Some((value, consumed))
}

/// Read an unsigned varint32.
/// Returns (value, bytes_consumed) or None if invalid.
fn read_unsigned_varint32(data: &[u8]) -> Option<(u32, usize)> {
    let mut value: u32 = 0;
    let mut shift = 0;
    for (i, &byte) in data.iter().enumerate() {
        value |= ((byte & 0x7F) as u32) << shift;
        if byte & 0x80 == 0 {
            return Some((value, i + 1));
        }
        shift += 7;
        if shift >= 32 {
            return None; // Overflow
        }
    }
    None // Ran out of data
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk_creation() {
        let chunk = Chunk::new(1, -2);
        assert_eq!(chunk.x, 1);
        assert_eq!(chunk.z, -2);
        assert_eq!(chunk.sub_chunks.len(), SUBCHUNK_COUNT as usize);
    }

    #[test]
    fn test_biome_encoding() {
        let chunk = Chunk::new(0, 0);
        let biomes = chunk.encode_biomes();
        // 24 sections * (1 header + 1 byte varint for biome ID 0) + 1 border byte = 49 bytes
        assert_eq!(biomes.len(), SUBCHUNK_COUNT as usize * 2 + 1);
        // Last byte should be 0 (no border blocks)
        assert_eq!(*biomes.last().unwrap(), 0);
    }

    #[test]
    fn test_heightmap_types() {
        let mut hm = HeightMap::new();
        for z in 0u8..16 {
            for x in 0u8..16 {
                hm.set(x, z, 16);
            }
        }

        // Subchunk y=0 (0..15): height 16 is above.
        assert_eq!(hm.for_subchunk(0).0, HeightMapType::TooHigh);

        // Subchunk y=1 (16..31): height 16 is within.
        let (ty, data) = hm.for_subchunk(1);
        assert_eq!(ty, HeightMapType::HasData);
        let data = data.expect("heightmap data");
        assert_eq!(data[0], 0);

        // Subchunk y=2 (32..47): height 16 is below.
        assert_eq!(hm.for_subchunk(2).0, HeightMapType::TooLow);
    }

    #[test]
    fn test_fill_floor() {
        let mut chunk = Chunk::new(0, 0);
        chunk.fill_floor(3, blocks::GRASS_BLOCK);

        // Subchunk 4 should no longer be empty
        assert!(!chunk.sub_chunks[4].is_empty());

        // Subchunk 3 should still be empty (below Y=0)
        assert!(chunk.sub_chunks[3].is_empty());
    }

    #[test]
    fn test_subchunk_encoding() {
        let mut chunk = Chunk::new(0, 0);
        chunk.fill_floor(1, blocks::GRASS_BLOCK);

        // Y index 0 corresponds to subchunk index 4
        let data = chunk.encode_subchunk(0);
        assert!(data.is_some());
        let data = data.unwrap();

        // Version 9, 1 storage, Y index 0
        assert_eq!(data[0], 9);
        assert_eq!(data[1], 1);
        assert_eq!(data[2] as i8, 0);
    }
}
