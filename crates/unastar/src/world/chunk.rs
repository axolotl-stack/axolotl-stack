//! Chunk data structures and encoding - Bedrock protocol format.
//!
//! Uses protocol 2168's request mode (`subchunks_count = 0` with an optional
//! sub-chunk limit) to let clients request sub-chunks on demand.

use bytes::{BufMut, BytesMut};

use crate::world::generator::{NativeBiome, QuartBiomes};

/// Legacy sub-chunk request-mode sentinels retained for callers targeting
/// older Valentine protocol versions. Protocol 2168 no longer encodes these
/// negative values in `LevelChunkPacket`; its request mode is represented by
/// `subchunks_count = 0` and `client_request_sub_chunk_limit`.
pub mod request_mode {
    /// Client requests sub-chunks on demand via `SubChunkRequest`.
    pub const LIMITED: i32 = -2;

    /// Legacy mode: all sub-chunks sent inline.
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

/// Biome of a new chunk before a generator sets one: Bedrock `plains`.
pub const DEFAULT_BIOME_ID: u32 = 1;

/// Bytes of the height map at the start of a Data3D value (256 x i16); every
/// Bedrock Data3D value is at least this long.
pub const DATA3D_HEIGHTMAP_LEN: usize = 512;

/// Data3D biome section header meaning "same biomes as the section below".
const DATA3D_COPY_PREVIOUS: u8 = 0xFF;

/// Block runtime IDs fetched from the shared vanilla block registry.
/// Uses default runtime state IDs from normalized generated block data.
pub mod blocks {
    use crate::registry::BlockRegistry;
    use std::collections::HashMap;
    use std::sync::LazyLock;

    /// Lookup a block's default state ID by string ID.
    fn lookup(name: &str) -> u32 {
        let registry = BlockRegistry::vanilla();
        registry
            .default_state_id_by_name(name)
            .or_else(|| registry.default_state_id_by_name("minecraft:air"))
            .unwrap_or(0)
    }

    /// Pre-built block name -> ID lookup map for surface rules.
    /// This provides O(1) lookup for generated surface rules.
    static BLOCK_LOOKUP: LazyLock<HashMap<String, u32>> = LazyLock::new(|| {
        let mut map = HashMap::new();
        for block in BlockRegistry::vanilla().iter() {
            map.insert(block.string_id.clone(), block.default_state_id);
        }
        map
    });

    /// Get block ID by name. Returns AIR if not found.
    ///
    /// This is the primary lookup function for surface rules.
    /// It uses a pre-built HashMap for O(1) lookup.
    #[inline]
    pub fn get_block_id(name: &str) -> u32 {
        BLOCK_LOOKUP.get(name).copied().unwrap_or(*AIR)
    }

    // Core blocks
    pub static AIR: LazyLock<u32> = LazyLock::new(|| lookup("minecraft:air"));
    pub static STONE: LazyLock<u32> = LazyLock::new(|| lookup("minecraft:stone"));
    pub static DIRT: LazyLock<u32> = LazyLock::new(|| lookup("minecraft:dirt"));
    pub static GRASS_BLOCK: LazyLock<u32> = LazyLock::new(|| lookup("minecraft:grass_block"));
    pub static BEDROCK: LazyLock<u32> = LazyLock::new(|| lookup("minecraft:bedrock"));

    // Water and sand
    pub static WATER: LazyLock<u32> = LazyLock::new(|| lookup("minecraft:water"));
    pub static LAVA: LazyLock<u32> = LazyLock::new(|| lookup("minecraft:lava"));
    pub static SAND: LazyLock<u32> = LazyLock::new(|| lookup("minecraft:sand"));
    pub static GRAVEL: LazyLock<u32> = LazyLock::new(|| lookup("minecraft:gravel"));

    // Trees - Oak
    pub static OAK_LOG: LazyLock<u32> = LazyLock::new(|| lookup("minecraft:oak_log"));
    pub static OAK_LEAVES: LazyLock<u32> = LazyLock::new(|| lookup("minecraft:oak_leaves"));

    // Trees - Birch
    pub static BIRCH_LOG: LazyLock<u32> = LazyLock::new(|| lookup("minecraft:birch_log"));
    pub static BIRCH_LEAVES: LazyLock<u32> = LazyLock::new(|| lookup("minecraft:birch_leaves"));

    // Trees - Spruce
    pub static SPRUCE_LOG: LazyLock<u32> = LazyLock::new(|| lookup("minecraft:spruce_log"));
    pub static SPRUCE_LEAVES: LazyLock<u32> = LazyLock::new(|| lookup("minecraft:spruce_leaves"));
    pub static SPRUCE_PLANKS: LazyLock<u32> = LazyLock::new(|| lookup("minecraft:spruce_planks"));

    // Mountain blocks
    pub static SNOW_BLOCK: LazyLock<u32> = LazyLock::new(|| lookup("minecraft:snow"));
    pub static PACKED_ICE: LazyLock<u32> = LazyLock::new(|| lookup("minecraft:packed_ice"));
    pub static COBBLESTONE: LazyLock<u32> = LazyLock::new(|| lookup("minecraft:cobblestone"));

    // Desert/Mesa
    pub static SANDSTONE: LazyLock<u32> = LazyLock::new(|| lookup("minecraft:sandstone"));
    pub static RED_SAND: LazyLock<u32> = LazyLock::new(|| lookup("minecraft:red_sand"));
    pub static TERRACOTTA: LazyLock<u32> = LazyLock::new(|| lookup("minecraft:terracotta"));

    // Flowers and plants
    pub static GRASS: LazyLock<u32> = LazyLock::new(|| lookup("minecraft:short_grass"));
    pub static TALL_GRASS: LazyLock<u32> = LazyLock::new(|| lookup("minecraft:tall_grass"));
    pub static DANDELION: LazyLock<u32> = LazyLock::new(|| lookup("minecraft:dandelion"));
    pub static POPPY: LazyLock<u32> = LazyLock::new(|| lookup("minecraft:poppy"));
    pub static CORNFLOWER: LazyLock<u32> = LazyLock::new(|| lookup("minecraft:cornflower"));
    pub static OXEYE_DAISY: LazyLock<u32> = LazyLock::new(|| lookup("minecraft:oxeye_daisy"));
    pub static AZURE_BLUET: LazyLock<u32> = LazyLock::new(|| lookup("minecraft:azure_bluet"));
    pub static LILY_OF_THE_VALLEY: LazyLock<u32> =
        LazyLock::new(|| lookup("minecraft:lily_of_the_valley"));

    // Clay for rivers
    pub static CLAY: LazyLock<u32> = LazyLock::new(|| lookup("minecraft:clay"));

    // Trees - Jungle
    pub static JUNGLE_LOG: LazyLock<u32> = LazyLock::new(|| lookup("minecraft:jungle_log"));
    pub static JUNGLE_LEAVES: LazyLock<u32> = LazyLock::new(|| lookup("minecraft:jungle_leaves"));

    // Trees - Dark Oak
    pub static DARK_OAK_LOG: LazyLock<u32> = LazyLock::new(|| lookup("minecraft:dark_oak_log"));
    pub static DARK_OAK_LEAVES: LazyLock<u32> =
        LazyLock::new(|| lookup("minecraft:dark_oak_leaves"));

    // Trees - Acacia
    pub static ACACIA_LOG: LazyLock<u32> = LazyLock::new(|| lookup("minecraft:acacia_log"));
    pub static ACACIA_LEAVES: LazyLock<u32> = LazyLock::new(|| lookup("minecraft:acacia_leaves"));

    // Swamp
    pub static VINE: LazyLock<u32> = LazyLock::new(|| lookup("minecraft:vine"));
    pub static LILY_PAD: LazyLock<u32> = LazyLock::new(|| lookup("minecraft:waterlily"));

    // Mushrooms
    pub static RED_MUSHROOM: LazyLock<u32> = LazyLock::new(|| lookup("minecraft:red_mushroom"));
    pub static BROWN_MUSHROOM: LazyLock<u32> = LazyLock::new(|| lookup("minecraft:brown_mushroom"));

    // Coarse dirt for savanna
    pub static COARSE_DIRT: LazyLock<u32> = LazyLock::new(|| lookup("minecraft:coarse_dirt"));

    // Stone variants (underground variety)
    pub static GRANITE: LazyLock<u32> = LazyLock::new(|| lookup("minecraft:granite"));
    pub static DIORITE: LazyLock<u32> = LazyLock::new(|| lookup("minecraft:diorite"));
    pub static ANDESITE: LazyLock<u32> = LazyLock::new(|| lookup("minecraft:andesite"));
    pub static DEEPSLATE: LazyLock<u32> = LazyLock::new(|| lookup("minecraft:deepslate"));
    pub static TUFF: LazyLock<u32> = LazyLock::new(|| lookup("minecraft:tuff"));

    // Ores
    pub static COAL_ORE: LazyLock<u32> = LazyLock::new(|| lookup("minecraft:coal_ore"));
    pub static IRON_ORE: LazyLock<u32> = LazyLock::new(|| lookup("minecraft:iron_ore"));
    pub static COPPER_ORE: LazyLock<u32> = LazyLock::new(|| lookup("minecraft:copper_ore"));
    pub static GOLD_ORE: LazyLock<u32> = LazyLock::new(|| lookup("minecraft:gold_ore"));
    pub static REDSTONE_ORE: LazyLock<u32> = LazyLock::new(|| lookup("minecraft:redstone_ore"));
    pub static LAPIS_ORE: LazyLock<u32> = LazyLock::new(|| lookup("minecraft:lapis_ore"));
    pub static DIAMOND_ORE: LazyLock<u32> = LazyLock::new(|| lookup("minecraft:diamond_ore"));
    pub static EMERALD_ORE: LazyLock<u32> = LazyLock::new(|| lookup("minecraft:emerald_ore"));

    // Deepslate ores
    pub static DEEPSLATE_COAL_ORE: LazyLock<u32> =
        LazyLock::new(|| lookup("minecraft:deepslate_coal_ore"));
    pub static DEEPSLATE_IRON_ORE: LazyLock<u32> =
        LazyLock::new(|| lookup("minecraft:deepslate_iron_ore"));
    pub static DEEPSLATE_COPPER_ORE: LazyLock<u32> =
        LazyLock::new(|| lookup("minecraft:deepslate_copper_ore"));
    pub static DEEPSLATE_GOLD_ORE: LazyLock<u32> =
        LazyLock::new(|| lookup("minecraft:deepslate_gold_ore"));
    pub static DEEPSLATE_REDSTONE_ORE: LazyLock<u32> =
        LazyLock::new(|| lookup("minecraft:deepslate_redstone_ore"));
    pub static DEEPSLATE_LAPIS_ORE: LazyLock<u32> =
        LazyLock::new(|| lookup("minecraft:deepslate_lapis_ore"));
    pub static DEEPSLATE_DIAMOND_ORE: LazyLock<u32> =
        LazyLock::new(|| lookup("minecraft:deepslate_diamond_ore"));
    pub static DEEPSLATE_EMERALD_ORE: LazyLock<u32> =
        LazyLock::new(|| lookup("minecraft:deepslate_emerald_ore"));

    // Raw ore blocks (for large ore veins)
    pub static RAW_COPPER_BLOCK: LazyLock<u32> =
        LazyLock::new(|| lookup("minecraft:raw_copper_block"));
    pub static RAW_IRON_BLOCK: LazyLock<u32> = LazyLock::new(|| lookup("minecraft:raw_iron_block"));
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

    /// Update heightmap for multiple columns using SIMD (x86_64).
    #[cfg(target_arch = "x86_64")]
    pub fn update_columns_simd(&mut self, x_start: u8, z: u8, heights: [i16; 8]) {
        unsafe {
            use std::arch::x86_64::*;

            let heights_simd = _mm_loadu_si128(heights.as_ptr() as *const __m128i);

            // We update 8 columns starting at x_start
            // These are contiguous in memory: (z<<4)|x ... (z<<4)|(x+7)
            let idx = ((z as usize) << 4) | (x_start as usize);

            let current_simd = _mm_loadu_si128(self.data.as_ptr().add(idx) as *const __m128i);

            // Calculate max
            let max_simd = _mm_max_epi16(heights_simd, current_simd);

            _mm_storeu_si128(self.data.as_mut_ptr().add(idx) as *mut __m128i, max_simd);
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
    /// Biome ids per sub-chunk, 16x16x16 like Bedrock's 3D biome storage.
    biomes: Vec<PalettedStorage>,
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

/// Valid bit widths for block storage (matches Bedrock protocol).
/// Note: 7 is skipped because 32/7 = 4.57 wastes too much padding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PaletteSize {
    Bits0 = 0,   // Single value, no indices
    Bits1 = 1,   // 2 palette entries max
    Bits2 = 2,   // 4 palette entries max
    Bits3 = 3,   // 8 palette entries max
    Bits4 = 4,   // 16 palette entries max
    Bits5 = 5,   // 32 palette entries max
    Bits6 = 6,   // 64 palette entries max
    Bits8 = 8,   // 256 palette entries max
    Bits16 = 16, // 65536 palette entries max
}

impl PaletteSize {
    /// Number of u32 words needed for 4096 block indices.
    pub const fn word_count(self) -> usize {
        match self {
            Self::Bits0 => 0,
            Self::Bits1 => 128,   // 4096 / 32
            Self::Bits2 => 256,   // 4096 / 16
            Self::Bits3 => 410,   // ceil(4096 / 10) - 10 indices per word
            Self::Bits4 => 512,   // 4096 / 8
            Self::Bits5 => 683,   // ceil(4096 / 6) - 6 indices per word
            Self::Bits6 => 820,   // ceil(4096 / 5) - 5 indices per word
            Self::Bits8 => 1024,  // 4096 / 4
            Self::Bits16 => 2048, // 4096 / 2
        }
    }

    /// Indices that fit per u32 word.
    pub const fn indices_per_word(self) -> usize {
        match self {
            Self::Bits0 => 0,
            Self::Bits1 => 32,
            Self::Bits2 => 16,
            Self::Bits3 => 10,
            Self::Bits4 => 8,
            Self::Bits5 => 6,
            Self::Bits6 => 5,
            Self::Bits8 => 4,
            Self::Bits16 => 2,
        }
    }

    /// Minimum size needed for a palette length.
    pub fn from_palette_len(len: usize) -> Self {
        match len {
            0..=1 => Self::Bits0,
            2 => Self::Bits1,
            3..=4 => Self::Bits2,
            5..=8 => Self::Bits3,
            9..=16 => Self::Bits4,
            17..=32 => Self::Bits5,
            33..=64 => Self::Bits6,
            65..=256 => Self::Bits8,
            _ => Self::Bits16,
        }
    }

    /// Parse from wire format bits value.
    pub fn from_bits(bits: u8) -> Option<Self> {
        match bits {
            0 => Some(Self::Bits0),
            1 => Some(Self::Bits1),
            2 => Some(Self::Bits2),
            3 => Some(Self::Bits3),
            4 => Some(Self::Bits4),
            5 => Some(Self::Bits5),
            6 => Some(Self::Bits6),
            8 => Some(Self::Bits8),
            16 => Some(Self::Bits16),
            _ => None,
        }
    }
}

/// Bit-packed block storage using variable-width indices.
///
/// Memory usage (indices only):
/// - 0 bits: 0 bytes (single block type)
/// - 4 bits: 2,048 bytes (typical terrain)
/// - 8 bits: 4,096 bytes (complex areas)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PalettedStorage {
    /// Palette of unique runtime block IDs.
    palette: Vec<u32>,
    /// Current bit width (determines storage format).
    size: PaletteSize,
    /// Bit-packed indices into palette. Empty when size == Bits0.
    indices: Vec<u32>,
}

impl Chunk {
    /// Create a new empty chunk at the given coordinates.
    pub fn new(x: i32, z: i32) -> Self {
        let sub_chunks = (0..SUBCHUNK_COUNT).map(|_| SubChunk::empty()).collect();

        let biomes = (0..SUBCHUNK_COUNT)
            .map(|_| PalettedStorage::single_block(DEFAULT_BIOME_ID))
            .collect();

        Self {
            x,
            z,
            sub_chunks,
            biomes,
            height_map: HeightMap::new(),
            recalculate_height_map: false,
        }
    }

    /// Build a chunk from a flat block array indexed `((y - MIN_Y) * 16 + z) * 16 + x`
    /// (`SUBCHUNK_COUNT * 4096` entries); `air` is the air runtime id for the height map.
    pub fn from_dense(x: i32, z: i32, blocks: &[u32], air: u32) -> Self {
        assert_eq!(
            blocks.len(),
            SUBCHUNK_COUNT as usize * BLOCKS_PER_SUBCHUNK,
            "dense chunk has the wrong size"
        );
        let sub_chunks = blocks
            .chunks_exact(BLOCKS_PER_SUBCHUNK)
            .map(|section| SubChunk {
                storage: PalettedStorage::from_dense_yzx(section),
            })
            .collect();
        let mut height_map = HeightMap::new();
        for lz in 0..16usize {
            for lx in 0..16usize {
                let top = (0..SUBCHUNK_COUNT as usize * 16)
                    .rev()
                    .find(|&y| blocks[(y * 16 + lz) * 16 + lx] != air);
                if let Some(y) = top {
                    height_map.set(lx as u8, lz as u8, (y as i32 + MIN_Y + 1) as i16);
                }
            }
        }
        let biomes = (0..SUBCHUNK_COUNT)
            .map(|_| PalettedStorage::single_block(DEFAULT_BIOME_ID))
            .collect();
        Self {
            x,
            z,
            sub_chunks,
            biomes,
            height_map,
            recalculate_height_map: false,
        }
    }

    /// Fill the bottom layers with blocks.
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

        // Update heightmap to reflect the topmost layer if non-air
        if layers > 0 && block_id != *blocks::AIR {
            let top_y = (layers - 1) as i16 + 1; // Height is Y of first air above highest block
            for z in 0u8..16 {
                for x in 0u8..16 {
                    let current = self.height_map.at(x, z);
                    if top_y > current {
                        self.height_map.set(x, z, top_y);
                    }
                }
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
            if block_id != *blocks::AIR {
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

    /// Get the count of sub-chunks from bottom to highest non-empty.
    /// Returns 0 if all are empty, otherwise returns highest_index + 1.
    pub fn highest_subchunk(&self) -> u16 {
        for (i, sub) in self.sub_chunks.iter().enumerate().rev() {
            if !sub.is_empty() {
                return (i + 1) as u16; // Return count, not index
            }
        }
        0
    }

    /// Set one biome id for the whole chunk.
    /// This affects grass/foliage color tinting on the client.
    pub fn set_biome(&mut self, biome_id: u32) {
        for section in &mut self.biomes {
            *section = PalettedStorage::single_block(biome_id);
        }
    }

    /// Bedrock biome id at a block (local x/z, absolute y); `None` outside the height range.
    pub fn biome_at(&self, x: u8, y: i16, z: u8) -> Option<u32> {
        let adjusted_y = (y as i32) - MIN_Y;
        if adjusted_y < 0 || adjusted_y >= (SUBCHUNK_COUNT as i32 * 16) {
            return None;
        }
        let section = &self.biomes[(adjusted_y >> 4) as usize];
        Some(section.get_block(x as usize & 15, adjusted_y as usize & 15, z as usize & 15))
    }

    /// Set every block's biome from quart-resolution (4x4x4) generator biomes.
    pub fn set_biomes_from_quarts(
        &mut self,
        quarts: &QuartBiomes,
        mut to_bedrock: impl FnMut(NativeBiome) -> u32,
    ) {
        for (index, section) in self.biomes.iter_mut().enumerate() {
            let mut ids = [0u32; 64];
            for qy in 0..4 {
                for qz in 0..4 {
                    for qx in 0..4 {
                        ids[(qy * 4 + qz) * 4 + qx] =
                            to_bedrock(quarts.get(qx, index * 4 + qy, qz));
                    }
                }
            }
            if ids.iter().all(|&id| id == ids[0]) {
                *section = PalettedStorage::single_block(ids[0]);
                continue;
            }
            let mut dense = [0u32; BLOCKS_PER_SUBCHUNK];
            for (i, slot) in dense.iter_mut().enumerate() {
                let (y, z, x) = (i >> 8, (i >> 4) & 15, i & 15);
                *slot = ids[((y >> 2) * 4 + (z >> 2)) * 4 + (x >> 2)];
            }
            *section = PalettedStorage::from_dense_yzx(&dense);
        }
    }

    /// Encode biome data only for protocol 2168 request-mode LevelChunk
    /// packets. The caller supplies the zero sub-chunk count and optional
    /// limit; this payload contains the biome sections and border count.
    pub fn encode_biomes(&self) -> Vec<u8> {
        let mut buf = BytesMut::with_capacity(SUBCHUNK_COUNT as usize * 2 + 1);

        // One network-format paletted storage per section, like block storage.
        for section in &self.biomes {
            section.encode(&mut buf);
        }

        // Border blocks count (0 = none)
        buf.put_u8(0);

        buf.to_vec()
    }

    /// Encode biomes as Bedrock's on-disk Data3D value (LevelDB key tag `0x2B`).
    ///
    /// Layout, as found in BDS-generated worlds (see `decode_biomes` in
    /// `examples/bds_worldgen_reference.rs`):
    /// - 256 i16 LE heights indexed `x + z * 16`, each the first air Y above the
    ///   column relative to [`MIN_Y`];
    /// - one biome storage per sub-chunk from the bottom: header `(bits << 1) | 1`
    ///   (BDS sets the low bit), the packed u32 LE index words, the palette length
    ///   as i32 LE and the palette as i32 LE biome ids. A single-value storage
    ///   (`bits == 0`) is the header plus one i32 id, without a length. Header
    ///   `0xFF` repeats the section below; BDS uses it for identical sections.
    pub fn encode_biomes_disk(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(DATA3D_HEIGHTMAP_LEN + SUBCHUNK_COUNT as usize * 5);
        for height in self.height_map.data {
            out.extend_from_slice(&(height - MIN_Y as i16).to_le_bytes());
        }
        let mut previous: Option<&PalettedStorage> = None;
        for section in &self.biomes {
            if previous == Some(section) {
                out.push(DATA3D_COPY_PREVIOUS);
            } else {
                section.encode_disk_biomes(&mut out);
            }
            previous = Some(section);
        }
        out
    }

    /// Replace this chunk's biomes with a Data3D value from [`Self::encode_biomes_disk`]
    /// or BDS. The height map is skipped (heights come from the blocks); when fewer
    /// than [`SUBCHUNK_COUNT`] sections are present the last one repeats upward.
    /// On error the chunk's biomes are unchanged.
    pub fn decode_biomes_disk(&mut self, data: &[u8]) -> Result<(), String> {
        let mut rest = data.get(DATA3D_HEIGHTMAP_LEN..).ok_or_else(|| {
            format!(
                "Data3D value is {} bytes, shorter than its {DATA3D_HEIGHTMAP_LEN}-byte height map",
                data.len()
            )
        })?;
        let mut sections: Vec<PalettedStorage> = Vec::with_capacity(SUBCHUNK_COUNT as usize);
        while let Some(&header) = rest.first() {
            if sections.len() == SUBCHUNK_COUNT as usize {
                return Err(format!(
                    "Data3D value has {} bytes after {SUBCHUNK_COUNT} biome sections",
                    rest.len()
                ));
            }
            let section = if header == DATA3D_COPY_PREVIOUS {
                rest = &rest[1..];
                sections
                    .last()
                    .cloned()
                    .ok_or("Data3D first biome section is a copy marker")?
            } else {
                let (section, used) = PalettedStorage::decode_disk_biomes(rest)
                    .map_err(|e| format!("Data3D biome section {}: {e}", sections.len()))?;
                rest = &rest[used..];
                section
            };
            sections.push(section);
        }
        let last = sections
            .last()
            .cloned()
            .ok_or("Data3D value has no biome sections")?;
        sections.resize(SUBCHUNK_COUNT as usize, last);
        self.biomes = sections;
        Ok(())
    }

    /// Replace this chunk's biomes with exactly the layout of [`Self::encode_biomes`]:
    /// [`SUBCHUNK_COUNT`] network-format storages, a zero border-block count, and no
    /// other bytes. On error the chunk's biomes are unchanged.
    pub fn decode_biomes_network(&mut self, data: &[u8]) -> Result<(), String> {
        let mut rest = data;
        let mut sections = Vec::with_capacity(SUBCHUNK_COUNT as usize);
        for index in 0..SUBCHUNK_COUNT {
            let (section, used) = PalettedStorage::decode(rest)
                .map_err(|e| format!("network biome section {index}: {e}"))?;
            rest = &rest[used..];
            sections.push(section);
        }
        if rest != [0] {
            return Err(format!(
                "expected only a zero border-block count after {SUBCHUNK_COUNT} network biome sections, found {} bytes",
                rest.len()
            ));
        }
        self.biomes = sections;
        Ok(())
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
            return *blocks::AIR;
        }

        let subchunk_idx = (adjusted_y / 16) as usize;
        let local_y = (adjusted_y % 16) as u8;

        self.sub_chunks
            .get(subchunk_idx)
            .map(|s| s.get_block(x, local_y, z))
            .unwrap_or(*blocks::AIR)
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

        if block_id != *blocks::AIR {
            // Non-air block: update height if higher than current
            if y >= current_height {
                self.height_map.set(x, z, y + 1);
            }
        } else if y + 1 == current_height {
            // Air block at the current height - need to recalculate
            // Scan downward to find new highest non-air block
            let mut new_height = MIN_Y as i16;
            for check_y in (MIN_Y as i16..y).rev() {
                if self.get_block(x, check_y, z) != *blocks::AIR {
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

        let offset = 3;

        // Decode first storage layer (we only use the first one)
        if storage_count > 0 && offset < data.len() {
            let storage = PalettedStorage::decode(&data[offset..])?;
            let _ = storage.1; // bytes consumed (only first layer used)

            // Replace the subchunk's storage with the decoded one
            self.sub_chunks[array_idx].storage = storage.0;
        }

        // Update heightmap for this subchunk
        let base_y = y_index * 16;
        for x in 0u8..16 {
            for z in 0u8..16 {
                for local_y in (0..16).rev() {
                    let block = self.sub_chunks[array_idx].get_block(x, local_y, z);
                    if block != *blocks::AIR {
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
            storage: PalettedStorage::single_block(*blocks::AIR),
        }
    }

    /// Check if this subchunk is all air.
    pub fn is_empty(&self) -> bool {
        self.storage.palette.len() == 1 && self.storage.palette[0] == *blocks::AIR
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
    /// Create storage with a single block type (0 bytes for indices).
    fn single_block(runtime_id: u32) -> Self {
        Self {
            palette: vec![runtime_id],
            size: PaletteSize::Bits0,
            indices: vec![],
        }
    }

    /// Pack 4096 blocks given in `(y, z, x)` order (y outermost) into the `(x, z, y)` index
    /// layout, with the smallest palette width that fits.
    fn from_dense_yzx(blocks: &[u32]) -> Self {
        debug_assert_eq!(blocks.len(), BLOCKS_PER_SUBCHUNK);
        let mut palette: Vec<u32> = Vec::new();
        let mut indices = [0u16; BLOCKS_PER_SUBCHUNK];
        let mut last = (u32::MAX, 0u16);
        for (i, &block) in blocks.iter().enumerate() {
            let index = if block == last.0 && !palette.is_empty() {
                last.1
            } else {
                let index = match palette.iter().position(|&p| p == block) {
                    Some(index) => index as u16,
                    None => {
                        palette.push(block);
                        (palette.len() - 1) as u16
                    }
                };
                last = (block, index);
                index
            };
            let (y, z, x) = (i >> 8, (i >> 4) & 15, i & 15);
            indices[(x << 8) | (z << 4) | y] = index;
        }
        let size = PaletteSize::from_palette_len(palette.len());
        if size == PaletteSize::Bits0 {
            return Self {
                palette,
                size,
                indices: vec![],
            };
        }
        let bits = size as usize;
        let per_word = size.indices_per_word();
        let mut words = vec![0u32; size.word_count()];
        for (i, &index) in indices.iter().enumerate() {
            words[i / per_word] |= (index as u32) << ((i % per_word) * bits);
        }
        Self {
            palette,
            size,
            indices: words,
        }
    }

    /// Fill an entire Y layer with a block.
    fn fill_layer(&mut self, y: usize, block_id: u32) {
        let palette_idx = self.get_or_add_palette_entry(block_id);

        // If still single-value, nothing more to do
        if self.size == PaletteSize::Bits0 {
            return;
        }

        // Fill the layer
        for x in 0..SUBCHUNK_SIZE {
            for z in 0..SUBCHUNK_SIZE {
                self.set_palette_index(x, y, z, palette_idx);
            }
        }
    }

    /// Get block runtime ID at local coordinates (0-15 each).
    #[inline]
    fn get_block(&self, x: usize, y: usize, z: usize) -> u32 {
        // Fast path: single-value palette
        if self.size == PaletteSize::Bits0 {
            return self.palette[0];
        }

        let palette_idx = self.get_palette_index(x, y, z);
        self.palette
            .get(palette_idx as usize)
            .copied()
            .unwrap_or(*blocks::AIR)
    }

    /// Get palette index using bit unpacking.
    #[inline]
    fn get_palette_index(&self, x: usize, y: usize, z: usize) -> u16 {
        // XZY order: (x << 8) | (z << 4) | y
        let block_idx = (x << 8) | (z << 4) | y;
        let bits = self.size as usize;
        let indices_per_word = self.size.indices_per_word();

        let word_idx = block_idx / indices_per_word;
        let idx_in_word = block_idx % indices_per_word;
        let bit_pos = idx_in_word * bits;

        let mask = (1u32 << bits) - 1;
        let word = self.indices.get(word_idx).copied().unwrap_or(0);
        ((word >> bit_pos) & mask) as u16
    }

    /// Set block runtime ID at local coordinates (0-15 each).
    /// Returns the previous runtime ID.
    fn set_block(&mut self, x: usize, y: usize, z: usize, block_id: u32) -> u32 {
        let old = self.get_block(x, y, z);
        if old == block_id {
            return old;
        }

        let palette_idx = self.get_or_add_palette_entry(block_id);

        // If still single-value, nothing more to do
        if self.size == PaletteSize::Bits0 {
            return old;
        }

        self.set_palette_index(x, y, z, palette_idx);
        old
    }

    /// Set palette index using bit packing.
    #[inline]
    fn set_palette_index(&mut self, x: usize, y: usize, z: usize, index: u16) {
        let block_idx = (x << 8) | (z << 4) | y;
        let bits = self.size as usize;
        let indices_per_word = self.size.indices_per_word();

        let word_idx = block_idx / indices_per_word;
        let idx_in_word = block_idx % indices_per_word;
        let bit_pos = idx_in_word * bits;

        let mask = (1u32 << bits) - 1;
        let word = &mut self.indices[word_idx];
        *word = (*word & !(mask << bit_pos)) | ((index as u32 & mask) << bit_pos);
    }

    /// Get palette index for a block, adding if needed. May trigger resize.
    fn get_or_add_palette_entry(&mut self, block_id: u32) -> u16 {
        // Check if already in palette
        for (i, &id) in self.palette.iter().enumerate() {
            if id == block_id {
                return i as u16;
            }
        }

        // Add new entry
        let new_idx = self.palette.len();
        self.palette.push(block_id);

        // Check if resize needed
        let needed_size = PaletteSize::from_palette_len(self.palette.len());
        if (needed_size as u8) > (self.size as u8) {
            self.grow_to(needed_size);
        }

        new_idx as u16
    }

    /// Grow storage to a larger bit width, preserving all data.
    fn grow_to(&mut self, new_size: PaletteSize) {
        let old_size = self.size;

        // Transitioning from single-value to multi-value
        if old_size == PaletteSize::Bits0 {
            self.size = new_size;
            self.indices = vec![0u32; new_size.word_count()];
            return;
        }

        // Copy all indices to new storage
        let old_indices = std::mem::take(&mut self.indices);
        let old_bits = old_size as usize;
        let old_ipw = old_size.indices_per_word();
        let old_mask = (1u32 << old_bits) - 1;

        self.size = new_size;
        self.indices = vec![0u32; new_size.word_count()];

        for block_idx in 0..BLOCKS_PER_SUBCHUNK {
            // Read from old
            let old_word_idx = block_idx / old_ipw;
            let old_idx_in_word = block_idx % old_ipw;
            let old_bit_pos = old_idx_in_word * old_bits;
            let old_word = old_indices.get(old_word_idx).copied().unwrap_or(0);
            let palette_idx = ((old_word >> old_bit_pos) & old_mask) as u16;

            // Write to new
            let x = block_idx >> 8;
            let z = (block_idx >> 4) & 0xF;
            let y = block_idx & 0xF;
            self.set_palette_index(x, y, z, palette_idx);
        }
    }

    /// Calculate bits needed per block index.
    #[allow(dead_code)]
    fn bits_per_block(&self) -> u8 {
        self.size as u8
    }

    /// Encode this storage for network transmission.
    fn encode(&self, buf: &mut BytesMut) {
        let bits = self.size as u8;

        // Network encoding header: (bits_per_block << 1) | 1
        buf.put_u8((bits << 1) | 1);

        if bits == 0 {
            // Single-value: just the palette entry as signed varint
            write_signed_varint32(buf, self.palette[0] as i32);
        } else {
            // Write packed u32 words in little-endian
            for &word in &self.indices {
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

        let size = PaletteSize::from_bits(bits)
            .ok_or_else(|| format!("Unsupported bit width: {}", bits))?;

        if bits == 0 {
            // Single-value palette
            let (value, consumed) =
                read_signed_varint32(&data[offset..]).ok_or("Failed to read palette value")?;
            offset += consumed;

            return Ok((
                Self {
                    palette: vec![value as u32],
                    size: PaletteSize::Bits0,
                    indices: vec![],
                },
                offset,
            ));
        }

        // Multi-value: read packed indices
        let word_count = size.word_count();
        let word_bytes = word_count * 4;

        if data.len() < offset + word_bytes {
            return Err("Not enough data for indices".to_string());
        }

        let mut indices = Vec::with_capacity(word_count);
        for i in 0..word_count {
            let wo = offset + i * 4;
            let word = u32::from_le_bytes([data[wo], data[wo + 1], data[wo + 2], data[wo + 3]]);
            indices.push(word);
        }
        offset += word_bytes;

        // Read palette
        let (palette_len, consumed) =
            read_signed_varint32(&data[offset..]).ok_or("Failed to read palette length")?;
        offset += consumed;

        let mut palette = Vec::with_capacity(palette_len as usize);
        for _ in 0..palette_len {
            let (value, consumed) =
                read_signed_varint32(&data[offset..]).ok_or("Failed to read palette entry")?;
            offset += consumed;
            palette.push(value as u32);
        }

        Ok((
            Self {
                palette,
                size,
                indices,
            },
            offset,
        ))
    }

    /// Append this storage in the Data3D biome format (see [`Chunk::encode_biomes_disk`]).
    fn encode_disk_biomes(&self, out: &mut Vec<u8>) {
        let bits = self.size as u8;
        out.push((bits << 1) | 1);
        if bits == 0 {
            out.extend_from_slice(&(self.palette[0] as i32).to_le_bytes());
            return;
        }
        for &word in &self.indices {
            out.extend_from_slice(&word.to_le_bytes());
        }
        out.extend_from_slice(&(self.palette.len() as i32).to_le_bytes());
        for &id in &self.palette {
            out.extend_from_slice(&(id as i32).to_le_bytes());
        }
    }

    /// Decode one Data3D biome storage (not a copy marker); returns (storage, bytes used).
    /// Either value of the header's low bit is accepted.
    fn decode_disk_biomes(data: &[u8]) -> Result<(Self, usize), String> {
        let header = *data.first().ok_or("missing storage header")?;
        let bits = header >> 1;
        let size =
            PaletteSize::from_bits(bits).ok_or_else(|| format!("unsupported bit width {bits}"))?;
        let mut offset = 1;
        if size == PaletteSize::Bits0 {
            let id = read_biome_id(data, &mut offset)?;
            return Ok((Self::single_block(id), offset));
        }

        let word_bytes = size.word_count() * 4;
        let words = data
            .get(offset..offset + word_bytes)
            .ok_or("truncated index words")?;
        let indices: Vec<u32> = words
            .chunks_exact(4)
            .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
            .collect();
        offset += word_bytes;

        let palette_len = read_i32_le(data, &mut offset)?;
        if palette_len < 1 || palette_len as u64 > 1u64 << bits {
            return Err(format!(
                "palette length {palette_len} does not fit {bits}-bit indices"
            ));
        }
        let palette = (0..palette_len)
            .map(|_| read_biome_id(data, &mut offset))
            .collect::<Result<Vec<u32>, String>>()?;

        let storage = Self {
            palette,
            size,
            indices,
        };
        // Out-of-palette indices would read as air in `get_block`; reject them here.
        for index in 0..BLOCKS_PER_SUBCHUNK {
            let (x, z, y) = (index >> 8, (index >> 4) & 0xF, index & 0xF);
            let palette_index = storage.get_palette_index(x, y, z);
            if palette_index as usize >= storage.palette.len() {
                return Err(format!(
                    "index {palette_index} outside palette of {}",
                    storage.palette.len()
                ));
            }
        }
        Ok((storage, offset))
    }
}

/// Read an i32 LE at `offset`, advancing it.
fn read_i32_le(data: &[u8], offset: &mut usize) -> Result<i32, String> {
    let bytes = data
        .get(*offset..*offset + 4)
        .ok_or("truncated i32 value")?;
    *offset += 4;
    Ok(i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

/// Read a Data3D palette entry (i32 LE biome id), rejecting negative ids.
fn read_biome_id(data: &[u8], offset: &mut usize) -> Result<u32, String> {
    let id = read_i32_le(data, offset)?;
    u32::try_from(id).map_err(|_| format!("negative biome id {id}"))
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
    fn from_dense_matches_block_by_block_construction() {
        let air = *blocks::AIR;
        let palette = [
            air,
            *blocks::STONE,
            *blocks::DIRT,
            *blocks::WATER,
            *blocks::DEEPSLATE,
        ];
        let mut dense = vec![air; SUBCHUNK_COUNT as usize * BLOCKS_PER_SUBCHUNK];
        let mut expected = Chunk::new(3, -2);
        let mut state = 0x9e37_79b9u32;
        for (i, slot) in dense.iter_mut().enumerate() {
            let y = (i >> 8) as i32;
            // Solid below y=200 with a few block kinds and holes, air above.
            if y < 200 {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                let block = palette[(state % palette.len() as u32) as usize];
                *slot = block;
                let (z, x) = (((i >> 4) & 15) as u8, (i & 15) as u8);
                expected.set_block(x, (y + MIN_Y) as i16, z, block);
            }
        }
        let packed = Chunk::from_dense(3, -2, &dense, air);
        for y in MIN_Y..MIN_Y + SUBCHUNK_COUNT as i32 * 16 {
            for z in 0..16u8 {
                for x in 0..16u8 {
                    assert_eq!(
                        packed.get_block(x, y as i16, z),
                        expected.get_block(x, y as i16, z)
                    );
                }
            }
        }
        for z in 0..16u8 {
            for x in 0..16u8 {
                assert_eq!(packed.height_map().at(x, z), expected.height_map().at(x, z));
            }
        }
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

    /// Data3D value: section 0 single-value biome 1, section 1 a copy marker,
    /// section 2 a 1-bit storage with biome 2 for x < 8 and 3 for x >= 8; the
    /// remaining sections are omitted.
    fn hand_made_data3d() -> Vec<u8> {
        let mut blob = vec![0u8; DATA3D_HEIGHTMAP_LEN];
        blob.push(1);
        blob.extend_from_slice(&1i32.to_le_bytes());
        blob.push(DATA3D_COPY_PREVIOUS);
        blob.push((1 << 1) | 1);
        // Index (x << 8) | (z << 4) | y, 32 per word: words 64.. cover x >= 8.
        for word in 0..128 {
            let bits: u32 = if word < 64 { 0 } else { u32::MAX };
            blob.extend_from_slice(&bits.to_le_bytes());
        }
        for value in [2i32, 2, 3] {
            blob.extend_from_slice(&value.to_le_bytes());
        }
        blob
    }

    #[test]
    fn data3d_decode_hand_made_blob_with_copy_marker() {
        let mut chunk = Chunk::new(0, 0);
        chunk
            .decode_biomes_disk(&hand_made_data3d())
            .expect("valid Data3D value");
        assert_eq!(chunk.biome_at(0, -64, 0), Some(1));
        assert_eq!(chunk.biome_at(15, -48, 15), Some(1), "copy of section 0");
        assert_eq!(chunk.biome_at(4, -33, 9), Some(1));
        assert_eq!(chunk.biome_at(7, -32, 15), Some(2));
        assert_eq!(chunk.biome_at(8, -17, 0), Some(3));
        // Missing sections repeat the last decoded one.
        assert_eq!(chunk.biome_at(0, 319, 9), Some(2));
        assert_eq!(chunk.biome_at(15, 100, 0), Some(3));
    }

    #[test]
    fn data3d_decode_rejects_malformed_values() {
        let blob = hand_made_data3d();
        let mut first_is_copy = vec![0u8; DATA3D_HEIGHTMAP_LEN];
        first_is_copy.push(DATA3D_COPY_PREVIOUS);
        let mut out_of_palette = blob.clone();
        let palette_len_at = blob.len() - 12;
        out_of_palette[palette_len_at..palette_len_at + 4].copy_from_slice(&1i32.to_le_bytes());
        out_of_palette.truncate(blob.len() - 4);

        let mut chunk = Chunk::new(0, 0);
        chunk.set_biome(9);
        for bad in [
            &blob[..blob.len() - 1],
            &blob[..DATA3D_HEIGHTMAP_LEN - 1],
            &blob[..DATA3D_HEIGHTMAP_LEN],
            &first_is_copy[..],
            &out_of_palette[..],
        ] {
            assert!(
                chunk.decode_biomes_disk(bad).is_err(),
                "{} bytes",
                bad.len()
            );
        }
        assert_eq!(
            chunk.biome_at(8, 0, 8),
            Some(9),
            "failed decode keeps biomes"
        );
    }

    #[test]
    fn data3d_round_trips_mixed_biomes_and_writes_heights() {
        let mut quarts = QuartBiomes::filled(1);
        for qz in 0..4 {
            for qx in 0..4 {
                for qy in 40..44 {
                    quarts.set(qx, qy, qz, 2);
                }
            }
        }
        quarts.set(3, 21, 0, 3);
        let mut chunk = Chunk::new(0, 0);
        chunk.set_biomes_from_quarts(&quarts, u32::from);
        chunk.set_block(3, 75, 5, *blocks::STONE);

        let data = chunk.encode_biomes_disk();
        let column = 3 + 5 * 16;
        assert_eq!(
            i16::from_le_bytes([data[2 * column], data[2 * column + 1]]),
            76 - MIN_Y as i16
        );
        assert_eq!(data[DATA3D_HEIGHTMAP_LEN + 5], DATA3D_COPY_PREVIOUS);

        let mut loaded = Chunk::new(0, 0);
        loaded
            .decode_biomes_disk(&data)
            .expect("own encoding decodes");
        assert_eq!(loaded.biome_at(12, 20, 0), Some(3));
        assert_eq!(loaded.biome_at(11, 20, 0), Some(1));
        assert_eq!(loaded.biome_at(0, 96, 15), Some(2));
        for x in 0..16 {
            for z in 0..16 {
                for y in MIN_Y as i16..MIN_Y as i16 + SUBCHUNK_COUNT as i16 * 16 {
                    assert_eq!(loaded.biome_at(x, y, z), chunk.biome_at(x, y, z));
                }
            }
        }
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
        chunk.fill_floor(3, *blocks::GRASS_BLOCK);

        // Subchunk 4 should no longer be empty
        assert!(!chunk.sub_chunks[4].is_empty());

        // Subchunk 3 should still be empty (below Y=0)
        assert!(chunk.sub_chunks[3].is_empty());
    }

    #[test]
    fn test_subchunk_encoding() {
        let mut chunk = Chunk::new(0, 0);
        chunk.fill_floor(1, *blocks::GRASS_BLOCK);

        // Y index 0 corresponds to subchunk index 4
        let data = chunk.encode_subchunk(0);
        assert!(data.is_some());
        let data = data.unwrap();

        // Version 9, 1 storage, Y index 0
        assert_eq!(data[0], 9);
        assert_eq!(data[1], 1);
        assert_eq!(data[2] as i8, 0);
    }

    #[test]
    fn test_palette_size_word_counts() {
        assert_eq!(PaletteSize::Bits0.word_count(), 0);
        assert_eq!(PaletteSize::Bits1.word_count(), 128);
        assert_eq!(PaletteSize::Bits4.word_count(), 512);
        assert_eq!(PaletteSize::Bits8.word_count(), 1024);
        assert_eq!(PaletteSize::Bits16.word_count(), 2048);
    }

    #[test]
    fn test_single_block_zero_indices() {
        let storage = PalettedStorage::single_block(*blocks::AIR);
        assert_eq!(storage.size, PaletteSize::Bits0);
        assert!(storage.indices.is_empty());
        assert_eq!(storage.get_block(0, 0, 0), *blocks::AIR);
        assert_eq!(storage.get_block(15, 15, 15), *blocks::AIR);
    }

    #[test]
    fn test_storage_growth() {
        let mut storage = PalettedStorage::single_block(*blocks::AIR);

        // Add second block type - should grow to Bits1
        storage.set_block(0, 0, 0, *blocks::STONE);
        assert_eq!(storage.size, PaletteSize::Bits1);
        assert_eq!(storage.indices.len(), 128); // 512 bytes

        // Verify both blocks readable
        assert_eq!(storage.get_block(0, 0, 0), *blocks::STONE);
        assert_eq!(storage.get_block(1, 0, 0), *blocks::AIR);
    }

    #[test]
    fn test_memory_reduction() {
        // Typical terrain: ~10 block types = 4 bits
        let mut storage = PalettedStorage::single_block(*blocks::AIR);
        for i in 0..10 {
            storage.set_block(i, 0, 0, i as u32);
        }

        assert_eq!(storage.size, PaletteSize::Bits4);
        // 512 words × 4 bytes = 2048 bytes (vs 8192 with Vec<u16>)
        assert_eq!(storage.indices.len() * 4, 2048);
    }

    #[test]
    fn test_encode_decode_roundtrip() {
        let mut storage = PalettedStorage::single_block(*blocks::AIR);
        storage.set_block(5, 5, 5, *blocks::STONE);
        storage.set_block(10, 10, 10, *blocks::DIRT);

        let mut buf = BytesMut::new();
        storage.encode(&mut buf);

        let (decoded, _) = PalettedStorage::decode(&buf).unwrap();

        assert_eq!(decoded.get_block(0, 0, 0), *blocks::AIR);
        assert_eq!(decoded.get_block(5, 5, 5), *blocks::STONE);
        assert_eq!(decoded.get_block(10, 10, 10), *blocks::DIRT);
    }

    #[test]
    fn test_all_palette_sizes() {
        // Test that we can write and read at various palette sizes
        for &(block_count, expected_size) in &[
            (1, PaletteSize::Bits0),
            (2, PaletteSize::Bits1),
            (4, PaletteSize::Bits2),
            (8, PaletteSize::Bits3),
            (16, PaletteSize::Bits4),
            (32, PaletteSize::Bits5),
            (64, PaletteSize::Bits6),
        ] {
            let mut storage = PalettedStorage::single_block(0);
            for i in 1..block_count {
                storage.set_block(i as usize % 16, (i as usize / 16) % 16, 0, i as u32);
            }
            assert_eq!(
                storage.size, expected_size,
                "Expected {:?} for {} blocks, got {:?}",
                expected_size, block_count, storage.size
            );

            // Verify all blocks can be read back
            for i in 1..block_count {
                let x = i as usize % 16;
                let y = (i as usize / 16) % 16;
                assert_eq!(storage.get_block(x, y, 0), i as u32);
            }
        }
    }

    #[test]
    fn test_fill_layer_bit_packed() {
        let mut storage = PalettedStorage::single_block(*blocks::AIR);

        // Fill layer 0 with stone
        storage.fill_layer(0, *blocks::STONE);

        // Check all blocks in layer 0 are stone
        for x in 0..16 {
            for z in 0..16 {
                assert_eq!(storage.get_block(x, 0, z), *blocks::STONE);
            }
        }

        // Check layer 1 is still air
        for x in 0..16 {
            for z in 0..16 {
                assert_eq!(storage.get_block(x, 1, z), *blocks::AIR);
            }
        }
    }
}
