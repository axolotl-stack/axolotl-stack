//! 3D terrain generation using density functions, aquifers, and surface rules.
//!
//! This module implements Java Edition-style terrain generation with:
//! - Density functions for 3D terrain shaping (overhangs, caves, etc.)
//! - Cell-based interpolation for performance
//! - Aquifer system for underground water/lava pockets
//! - Surface rules for biome-based block placement

use super::aquifer::{NoiseBasedAquifer, OverworldFluidPicker};
use super::constants::Biome;
use super::density::{build_overworld_router, NoiseChunk, NoiseRouter, WrapVisitor};
use super::noise::PerlinNoise;
use super::structures::{get_structure_pos, StructureConfig, StructureType};
use super::surface::{build_overworld_surface_rule, SurfaceSystem};
use super::xoroshiro::{JavaRandom, Xoroshiro128};
use crate::world::chunk::{blocks, Chunk};
use crate::world::generator::BiomeNoise;

/// Vanilla terrain generator using 3D density functions.
///
/// This generator produces terrain matching Java Edition's modern world generation:
/// - 3D density-based terrain with overhangs and natural caves
/// - Aquifer system for underground water and lava pockets
/// - Surface rules for biome-specific block placement
pub struct VanillaGenerator {
    /// World seed
    pub seed: i64,
    /// Biome/Climate noise system (MultiNoise)
    biome_noise: BiomeNoise,
    /// Detail noise for ore/stone variant placement
    detail_noise: PerlinNoise,
    /// Tree density noise
    tree_noise: PerlinNoise,
    /// Density function router for 3D terrain generation
    router: NoiseRouter,
    /// Surface rules system for biome-based surface blocks
    surface_system: SurfaceSystem,
}

impl VanillaGenerator {
    /// Sea level
    pub const SEA_LEVEL: i32 = 63;

    /// Create a new vanilla generator with the given seed.
    pub fn new(seed: i64) -> Self {
        let mut rng = Xoroshiro128::from_seed(seed);

        let biome_noise = BiomeNoise::from_seed(seed);
        let detail_noise = PerlinNoise::new(&mut rng);
        let tree_noise = PerlinNoise::new(&mut rng);

        // Build density function router for 3D terrain generation
        let router = build_overworld_router(seed);

        // Build surface rules system for biome-based surface blocks
        let surface_rule = build_overworld_surface_rule(seed);
        let surface_system = SurfaceSystem::new(seed, surface_rule, biome_noise.clone());

        Self {
            seed,
            biome_noise,
            detail_noise,
            tree_noise,
            router,
            surface_system,
        }
    }

    /// Get biome at position based on climate parameters.
    fn get_biome(&self, x: i32, z: i32) -> Biome {
        // Use Y=64 (sea level) for standard biome check
        self.biome_noise.get_biome(x, 64, z)
    }

    /// Find a safe spawn location by sampling terrain.
    ///
    /// Searches outward from origin for a location above sea level.
    pub fn find_safe_spawn(&self) -> (i32, i32, i32) {
        use super::density::SinglePointContext;

        for radius in 0i32..64 {
            for dx in -radius..=radius {
                for dz in -radius..=radius {
                    if dx.abs() != radius && dz.abs() != radius {
                        continue;
                    }

                    // Find surface by scanning down from max height
                    for y in (Self::SEA_LEVEL + 1..=128).rev() {
                        let ctx = SinglePointContext::new(dx, y, dz);
                        let density = self.router.final_density.compute(&ctx);

                        if density > 0.0 {
                            // Found solid block, spawn above it
                            return (dx, y + 2, dz);
                        }
                    }
                }
            }
        }

        // Default spawn above sea level
        (0, Self::SEA_LEVEL + 2, 0)
    }

    /// Map our biome enum to Bedrock biome IDs for grass/foliage color.
    fn to_bedrock_biome_id(biome: Biome) -> u32 {
        match biome {
            Biome::Ocean => 0,
            Biome::Plains => 1,
            Biome::Desert => 2,
            Biome::WindsweptHills => 3, // windswept_hills
            Biome::Forest => 4,
            Biome::Taiga => 5,
            Biome::Swamp => 6,
            Biome::River => 7,
            Biome::Beach => 16,
            Biome::BirchForest => 27,
            Biome::DarkForest => 29,
            Biome::SnowyTaiga => 30,
            Biome::Savanna => 35,
            Biome::Jungle => 21,
            Biome::Meadow => 177,
            Biome::FlowerForest => 132,
            Biome::SnowyMountains => 13,
            _ => 1, // Default to Plains for others
        }
    }

    /// Generate a chunk using 3D density functions.
    ///
    /// This is the Java Edition-style terrain generation using density functions
    /// and cell-based interpolation. It produces 3D features like overhangs and caves.
    ///
    /// The generation process:
    /// 1. Create a NoiseChunk for cell-based caching
    /// 2. Create aquifer system for underground water/lava pockets
    /// 3. Wire the router's density functions with cache implementations
    /// 4. Traverse cells (4x8x4 blocks) in X-outer, Z-middle, Y-inner order
    /// 5. Interpolate density at each block position
    /// 6. Use aquifer to determine fluid blocks
    /// 7. Place blocks based on density and aquifer output
    /// 8. Apply surface rules for biome-specific blocks
    pub fn generate_chunk(&self, chunk_x: i32, chunk_z: i32) -> Chunk {
        let mut chunk = Chunk::new(chunk_x, chunk_z);

        // Cell configuration matching Java Edition
        let cell_width = 4;  // blocks per cell in XZ
        let cell_height = 8; // blocks per cell in Y
        let min_y = -64;
        let height = 384;

        // Create NoiseChunk for this chunk
        let noise_chunk = NoiseChunk::new(
            chunk_x,
            chunk_z,
            cell_width,
            cell_height,
            min_y,
            height,
        );

        // Create aquifer system for underground water/lava pockets
        let fluid_picker = Box::new(OverworldFluidPicker::new(Self::SEA_LEVEL));
        let mut aquifer = NoiseBasedAquifer::new(
            chunk_x,
            chunk_z,
            min_y,
            height,
            self.router.barrier_noise.clone(),
            self.router.fluid_level_floodedness.clone(),
            self.router.fluid_level_spread.clone(),
            self.router.lava_noise.clone(),
            self.router.erosion.clone(),
            self.router.depth.clone(),
            self.seed,
            fluid_picker,
        );

        // Wire router functions with caching
        let _visitor = WrapVisitor::new(&noise_chunk);
        // Note: In a full implementation, we'd use router.map_all(&visitor)
        // to replace markers with cache implementations

        // Cell counts
        let cell_count_xz = 16 / cell_width;
        let cell_count_y = height / cell_height;

        // Initialize first X slice
        noise_chunk.initialize_for_first_cell_x();

        // Triple-nested loop over cells
        for cell_x in 0..cell_count_xz {
            noise_chunk.advance_cell_x(cell_x);

            for cell_z in 0..cell_count_xz {
                // Y descending for efficient surface detection
                for cell_y in (0..cell_count_y).rev() {
                    noise_chunk.select_cell_yz(cell_y, cell_z);

                    // Iterate blocks within cell (Y descending)
                    for y_in_cell in (0..cell_height).rev() {
                        let block_y = min_y + cell_y * cell_height + y_in_cell;
                        let t_y = y_in_cell as f64 / cell_height as f64;
                        noise_chunk.update_for_y(y_in_cell, t_y);

                        for x_in_cell in 0..cell_width {
                            let _block_x = chunk_x * 16 + cell_x * cell_width + x_in_cell;
                            let t_x = x_in_cell as f64 / cell_width as f64;
                            noise_chunk.update_for_x(x_in_cell, t_x);

                            for z_in_cell in 0..cell_width {
                                let _block_z = chunk_z * 16 + cell_z * cell_width + z_in_cell;
                                let t_z = z_in_cell as f64 / cell_width as f64;
                                noise_chunk.update_for_z(z_in_cell, t_z);

                                // Get density from router
                                let density = self.router.final_density.compute(&noise_chunk);

                                // Use aquifer to determine block state
                                let block = if let Some(fluid_block) = aquifer.compute_substance(&noise_chunk, density) {
                                    // Aquifer determined the block (air or fluid)
                                    fluid_block
                                } else if density > 0.0 {
                                    // Solid - use stone (surface rules will replace later)
                                    *blocks::STONE
                                } else {
                                    // This shouldn't happen since aquifer handles negative density
                                    *blocks::AIR
                                };

                                // Set block if not air
                                if block != *blocks::AIR {
                                    let local_x = (cell_x * cell_width + x_in_cell) as u8;
                                    let local_z = (cell_z * cell_width + z_in_cell) as u8;
                                    chunk.set_block(local_x, block_y as i16, local_z, block);
                                }
                            }
                        }
                    }
                }
            }

            noise_chunk.swap_slices();
        }

        noise_chunk.stop_interpolation();

        // Apply surface rules to replace stone with biome-appropriate surface blocks
        self.surface_system.build_surface(&mut chunk, chunk_x, chunk_z);

        /*
        // Add stone variants (granite, diorite, andesite, deepslate)
        self.add_stone_variants(&mut chunk, chunk_x, chunk_z);

        // Add ores
        self.add_ores(&mut chunk, chunk_x, chunk_z);

        // Carve caves and ravines
        self.carve_caves(&mut chunk, chunk_x, chunk_z);
        self.carve_ravines(&mut chunk, chunk_x, chunk_z);

        // Add trees
        if !self.has_structure_in_chunk(chunk_x, chunk_z) {
            self.add_trees(&mut chunk, chunk_x, chunk_z);
        }

        // Add vegetation (flowers, grass)
        self.add_vegetation(&mut chunk, chunk_x, chunk_z);

        // Add structures
        self.add_structures(&mut chunk, chunk_x, chunk_z);

        */
        // Sample center biome for the chunk (for grass/foliage color)
        let center_biome = self.get_biome(chunk_x * 16 + 8, chunk_z * 16 + 8);
        chunk.set_biome(Self::to_bedrock_biome_id(center_biome));

        chunk
    }

    /// Find the surface height at a given XZ position by scanning the chunk.
    ///
    /// Returns the Y coordinate of the highest solid block, or sea level if none found.
    fn find_surface_height(&self, chunk: &Chunk, local_x: u8, local_z: u8) -> i32 {
        // Scan from top down to find the first solid block
        for y in (Self::SEA_LEVEL..256).rev() {
            let block = chunk.get_block(local_x, y as i16, local_z);
            // Consider grass, dirt, sand, stone, etc. as surface
            if block != *blocks::AIR && block != *blocks::WATER {
                return y;
            }
        }
        Self::SEA_LEVEL
    }

    /// Add biome-appropriate trees with natural spacing.
    fn add_trees(&self, chunk: &mut Chunk, chunk_x: i32, chunk_z: i32) {
        // Larger step = fewer trees, more natural spacing
        for local_z in (1u8..15).step_by(5) {
            for local_x in (1u8..15).step_by(5) {
                let world_x = chunk_x * 16 + local_x as i32;
                let world_z = chunk_z * 16 + local_z as i32;

                // Use lower frequency for smoother tree distribution
                let tree_val =
                    self.tree_noise
                        .sample(world_x as f64 * 0.08, 0.0, world_z as f64 * 0.08);
                let biome = self.get_biome(world_x, world_z);

                // Tree density varies by biome - higher threshold = sparser trees
                let threshold = match biome {
                    Biome::Jungle | Biome::DarkForest => 0.2, // Dense but not overcrowded
                    Biome::Forest | Biome::BirchForest => 0.35, // Moderate density
                    Biome::FlowerForest => 0.4,
                    Biome::Taiga | Biome::SnowyTaiga => 0.45,
                    Biome::Swamp => 0.5,
                    Biome::Savanna => 0.65,
                    Biome::Plains | Biome::Meadow => 0.75, // Sparse trees on plains
                    _ => 1.0, // No trees in desert, beach, ocean, mountains, river
                };

                if tree_val < threshold {
                    continue;
                }

                // Find surface from chunk data instead of computing height
                let height = self.find_surface_height(chunk, local_x, local_z);
                if height <= Self::SEA_LEVEL || height > 95 {
                    continue;
                }

                // Biome-appropriate tree
                match biome {
                    Biome::Forest | Biome::Plains => {
                        self.place_oak_tree(chunk, local_x, height as i16, local_z)
                    }
                    Biome::BirchForest => {
                        self.place_birch_tree(chunk, local_x, height as i16, local_z)
                    }
                    Biome::Taiga | Biome::SnowyTaiga => {
                        self.place_spruce_tree(chunk, local_x, height as i16, local_z)
                    }
                    Biome::Jungle => self.place_jungle_tree(chunk, local_x, height as i16, local_z),
                    Biome::DarkForest => {
                        self.place_dark_oak_tree(chunk, local_x, height as i16, local_z)
                    }
                    Biome::Savanna => {
                        self.place_acacia_tree(chunk, local_x, height as i16, local_z)
                    }
                    Biome::Swamp => self.place_swamp_oak(chunk, local_x, height as i16, local_z),
                    Biome::FlowerForest => {
                        // Mix of oak and birch
                        if tree_val > 0.5 {
                            self.place_birch_tree(chunk, local_x, height as i16, local_z)
                        } else {
                            self.place_oak_tree(chunk, local_x, height as i16, local_z)
                        }
                    }
                    Biome::Meadow => {
                        // Rare single oak
                        if tree_val > 0.85 {
                            self.place_oak_tree(chunk, local_x, height as i16, local_z)
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    /// Place an oak tree (vanilla-accurate from WorldGenTrees.java)
    fn place_oak_tree(&self, chunk: &mut Chunk, x: u8, ground_y: i16, z: u8) {
        // Height: rand(3) + 4 = 4-6 blocks
        let height_seed = (self
            .seed
            .wrapping_add(x as i64 * 31)
            .wrapping_add(z as i64 * 17)) as u32;
        let trunk_height = ((height_seed % 3) + 4) as i16;

        // Place trunk
        for dy in 0..trunk_height {
            chunk.set_block(x, ground_y + dy, z, *blocks::OAK_LOG);
        }

        // Place leaves (vanilla: start 3 below top, go to top+1)
        let top_y = ground_y + trunk_height;
        for layer in 0i16..4 {
            let y = top_y - 3 + layer;
            // Radius: starts at 2, shrinks toward top
            // layer 0,1: radius 2; layer 2,3: radius 1
            let radius: i16 = if layer < 2 { 2 } else { 1 };

            for dx in -radius..=radius {
                for dz in -radius..=radius {
                    let nx = x as i16 + dx;
                    let nz = z as i16 + dz;

                    if nx < 0 || nx >= 16 || nz < 0 || nz >= 16 {
                        continue;
                    }

                    // Skip corners randomly for natural look (vanilla: abs(dx)==radius && abs(dz)==radius)
                    if dx.abs() == radius && dz.abs() == radius {
                        // Skip corners on lower layers randomly
                        let corner_hash =
                            ((height_seed as i16 + dx * 7 + dz * 13 + layer * 3) % 2) == 0;
                        if corner_hash && layer < 2 {
                            continue;
                        }
                    }

                    // Don't replace trunk
                    if dx == 0 && dz == 0 && layer < 3 {
                        continue;
                    }

                    chunk.set_block(nx as u8, y, nz as u8, *blocks::OAK_LEAVES);
                }
            }
        }
        // Top leaf
        chunk.set_block(x, top_y, z, *blocks::OAK_LEAVES);
    }

    /// Place a birch tree (vanilla-accurate, same shape as oak with birch blocks)
    fn place_birch_tree(&self, chunk: &mut Chunk, x: u8, ground_y: i16, z: u8) {
        // Birch is slightly taller: 5-7 blocks
        let height_seed = (self
            .seed
            .wrapping_add(x as i64 * 37)
            .wrapping_add(z as i64 * 23)) as u32;
        let trunk_height = ((height_seed % 3) + 5) as i16;

        for dy in 0..trunk_height {
            chunk.set_block(x, ground_y + dy, z, *blocks::BIRCH_LOG);
        }

        let top_y = ground_y + trunk_height;
        for layer in 0i16..4 {
            let y = top_y - 3 + layer;
            let radius: i16 = if layer < 2 { 2 } else { 1 };

            for dx in -radius..=radius {
                for dz in -radius..=radius {
                    let nx = x as i16 + dx;
                    let nz = z as i16 + dz;

                    if nx < 0 || nx >= 16 || nz < 0 || nz >= 16 {
                        continue;
                    }

                    if dx.abs() == radius && dz.abs() == radius {
                        let corner_hash =
                            ((height_seed as i16 + dx * 5 + dz * 11 + layer * 7) % 2) == 0;
                        if corner_hash && layer < 2 {
                            continue;
                        }
                    }

                    if dx == 0 && dz == 0 && layer < 3 {
                        continue;
                    }

                    chunk.set_block(nx as u8, y, nz as u8, *blocks::BIRCH_LEAVES);
                }
            }
        }
        chunk.set_block(x, top_y, z, *blocks::BIRCH_LEAVES);
    }

    /// Place a spruce tree (vanilla-accurate from WorldGenTaiga2.java - conical shape)
    fn place_spruce_tree(&self, chunk: &mut Chunk, x: u8, ground_y: i16, z: u8) {
        // Height: rand(4) + 6 = 6-9 blocks
        let height_seed = (self
            .seed
            .wrapping_add(x as i64 * 41)
            .wrapping_add(z as i64 * 29)) as u32;
        let trunk_height = ((height_seed % 4) + 6) as i16;

        // Bare trunk portion at bottom
        let bare_trunk = 1 + (height_seed % 2) as i16;
        // Leaves portion
        let leaves_height = trunk_height - bare_trunk;
        // Max leaf radius
        let max_radius = 2 + (height_seed % 2) as i16;

        // Place trunk
        for dy in 0..trunk_height {
            chunk.set_block(x, ground_y + dy, z, *blocks::SPRUCE_LOG);
        }

        // Conical leaves with oscillating pattern
        let mut current_radius = 0i16;
        let mut radius_grow = 1i16;
        let mut layer_in_tier = 0i16;

        for layer in 0..=leaves_height {
            let y = ground_y + trunk_height - layer;

            for dx in -current_radius..=current_radius {
                for dz in -current_radius..=current_radius {
                    let nx = x as i16 + dx;
                    let nz = z as i16 + dz;

                    if nx < 0 || nx >= 16 || nz < 0 || nz >= 16 {
                        continue;
                    }

                    // Skip corners unless radius is 0
                    if current_radius > 0
                        && dx.abs() == current_radius
                        && dz.abs() == current_radius
                    {
                        continue;
                    }

                    chunk.set_block(nx as u8, y, nz as u8, *blocks::SPRUCE_LEAVES);
                }
            }

            // Oscillating radius pattern like vanilla
            layer_in_tier += 1;
            if current_radius >= radius_grow {
                current_radius = 0;
                layer_in_tier = 0;
                radius_grow += 1;
                if radius_grow > max_radius {
                    radius_grow = max_radius;
                }
            } else {
                current_radius += 1;
            }
        }
    }

    /// Place generic leaf blob (unused fallback).
    fn place_leaves(&self, chunk: &mut Chunk, x: u8, top_y: i16, z: u8, leaf_block: u32) {
        for dy in -2i16..2 {
            let radius = if dy == 1 {
                1
            } else if dy >= -1 {
                2
            } else {
                1
            };
            for dx in -radius..=radius {
                for dz in -radius..=radius {
                    let nx = x as i16 + dx;
                    let nz = z as i16 + dz;
                    if nx < 0 || nx >= 16 || nz < 0 || nz >= 16 {
                        continue;
                    }
                    if dx == 0 && dz == 0 && dy < 0 {
                        continue;
                    }
                    if dx.abs() == radius && dz.abs() == radius && dy <= -1 {
                        continue;
                    }
                    chunk.set_block(nx as u8, top_y + dy, nz as u8, leaf_block);
                }
            }
        }
    }

    /// Place spruce-style conical leaves.
    fn place_spruce_leaves(&self, chunk: &mut Chunk, x: u8, top_y: i16, z: u8) {
        for dy in -4i16..2 {
            let radius = if dy >= 0 {
                0
            } else if dy == -1 {
                1
            } else {
                ((-dy) / 2).min(2)
            };
            for dx in -(radius as i16)..=(radius as i16) {
                for dz in -(radius as i16)..=(radius as i16) {
                    let nx = x as i16 + dx;
                    let nz = z as i16 + dz;
                    if nx < 0 || nx >= 16 || nz < 0 || nz >= 16 {
                        continue;
                    }
                    chunk.set_block(nx as u8, top_y + dy, nz as u8, *blocks::SPRUCE_LEAVES);
                }
            }
        }
        // Top
        chunk.set_block(x, top_y + 1, z, *blocks::SPRUCE_LEAVES);
    }

    /// Place a jungle tree (tall with large leaves).
    fn place_jungle_tree(&self, chunk: &mut Chunk, x: u8, ground_y: i16, z: u8) {
        let trunk_height = 8i16;
        for dy in 0..trunk_height {
            chunk.set_block(x, ground_y + dy, z, *blocks::JUNGLE_LOG);
        }
        // Large leaf canopy
        for dy in -3i16..2 {
            let radius = if dy >= 0 { 1 } else { 2 };
            for dx in -(radius as i16)..=(radius as i16) {
                for dz in -(radius as i16)..=(radius as i16) {
                    let nx = x as i16 + dx;
                    let nz = z as i16 + dz;
                    if nx < 0 || nx >= 16 || nz < 0 || nz >= 16 {
                        continue;
                    }
                    if dx == 0 && dz == 0 && dy < -1 {
                        continue;
                    }
                    chunk.set_block(
                        nx as u8,
                        ground_y + trunk_height - 1 + dy,
                        nz as u8,
                        *blocks::JUNGLE_LEAVES,
                    );
                }
            }
        }
    }

    /// Place a dark oak tree (short with wide canopy).
    fn place_dark_oak_tree(&self, chunk: &mut Chunk, x: u8, ground_y: i16, z: u8) {
        let trunk_height = 5i16;
        for dy in 0..trunk_height {
            chunk.set_block(x, ground_y + dy, z, *blocks::DARK_OAK_LOG);
        }
        // Very wide canopy
        for dy in -2i16..2 {
            let radius = if dy >= 0 { 2 } else { 3 };
            for dx in -(radius as i16)..=(radius as i16) {
                for dz in -(radius as i16)..=(radius as i16) {
                    let nx = x as i16 + dx;
                    let nz = z as i16 + dz;
                    if nx < 0 || nx >= 16 || nz < 0 || nz >= 16 {
                        continue;
                    }
                    if dx == 0 && dz == 0 && dy < 0 {
                        continue;
                    }
                    // Skip corners for round shape
                    if dx.abs() == radius && dz.abs() == radius {
                        continue;
                    }
                    chunk.set_block(
                        nx as u8,
                        ground_y + trunk_height - 1 + dy,
                        nz as u8,
                        *blocks::DARK_OAK_LEAVES,
                    );
                }
            }
        }
    }

    /// Place an acacia tree (bent trunk with flat top).
    fn place_acacia_tree(&self, chunk: &mut Chunk, x: u8, ground_y: i16, z: u8) {
        let trunk_height = 5i16;
        for dy in 0..trunk_height {
            chunk.set_block(x, ground_y + dy, z, *blocks::ACACIA_LOG);
        }
        // Flat canopy
        for dx in -2i16..=2 {
            for dz in -2i16..=2 {
                let nx = x as i16 + dx;
                let nz = z as i16 + dz;
                if nx < 0 || nx >= 16 || nz < 0 || nz >= 16 {
                    continue;
                }
                if dx.abs() == 2 && dz.abs() == 2 {
                    continue; // Skip corners
                }
                chunk.set_block(
                    nx as u8,
                    ground_y + trunk_height,
                    nz as u8,
                    *blocks::ACACIA_LEAVES,
                );
            }
        }
        // Top layer
        for dx in -1i16..=1 {
            for dz in -1i16..=1 {
                let nx = x as i16 + dx;
                let nz = z as i16 + dz;
                if nx >= 0 && nx < 16 && nz >= 0 && nz < 16 {
                    chunk.set_block(
                        nx as u8,
                        ground_y + trunk_height + 1,
                        nz as u8,
                        *blocks::ACACIA_LEAVES,
                    );
                }
            }
        }
    }

    /// Place a swamp oak with vines.
    fn place_swamp_oak(&self, chunk: &mut Chunk, x: u8, ground_y: i16, z: u8) {
        let trunk_height = 5i16;
        for dy in 0..trunk_height {
            chunk.set_block(x, ground_y + dy, z, *blocks::OAK_LOG);
        }
        self.place_leaves(
            chunk,
            x,
            ground_y + trunk_height - 1,
            z,
            *blocks::OAK_LEAVES,
        );

        // Add vines on sides
        for dy in 0i16..4 {
            let y = ground_y + trunk_height - 2 - dy;
            if x > 0 {
                chunk.set_block(x - 1, y, z, *blocks::VINE);
            }
            if x < 15 {
                chunk.set_block(x + 1, y, z, *blocks::VINE);
            }
            if z > 0 {
                chunk.set_block(x, y, z - 1, *blocks::VINE);
            }
            if z < 15 {
                chunk.set_block(x, y, z + 1, *blocks::VINE);
            }
        }
    }

    /// Add flowers and vegetation based on biome using noise for smooth distribution.
    fn add_vegetation(&self, chunk: &mut Chunk, chunk_x: i32, chunk_z: i32) {
        for local_z in 0u8..16 {
            for local_x in 0u8..16 {
                let world_x = chunk_x * 16 + local_x as i32;
                let world_z = chunk_z * 16 + local_z as i32;
                let biome = self.get_biome(world_x, world_z);
                let height = self.find_surface_height(chunk, local_x, local_z);

                if height <= Self::SEA_LEVEL {
                    continue;
                }

                let fx = world_x as f64;
                let fz = world_z as f64;

                // Use very low frequency for smooth, spread-out distribution
                let veg_noise = self.tree_noise.sample(fx * 0.02, 0.5, fz * 0.02);
                // Secondary noise for flower type variation
                let type_noise = self.detail_noise.sample(fx * 0.06, 0.0, fz * 0.06);

                match biome {
                    Biome::FlowerForest => {
                        // Smooth flower clusters
                        if veg_noise > 0.0 {
                            let flower = if type_noise < -0.3 {
                                *blocks::DANDELION
                            } else if type_noise < -0.1 {
                                *blocks::POPPY
                            } else if type_noise < 0.1 {
                                *blocks::CORNFLOWER
                            } else if type_noise < 0.3 {
                                *blocks::OXEYE_DAISY
                            } else if type_noise < 0.5 {
                                *blocks::AZURE_BLUET
                            } else {
                                *blocks::LILY_OF_THE_VALLEY
                            };
                            chunk.set_block(local_x, height as i16, local_z, flower);
                        }
                    }
                    Biome::Meadow => {
                        // Smooth flower and grass patches
                        if veg_noise > -0.2 {
                            let flower = if type_noise < -0.2 {
                                *blocks::DANDELION
                            } else if type_noise < 0.2 {
                                *blocks::CORNFLOWER
                            } else if type_noise < 0.4 {
                                *blocks::OXEYE_DAISY
                            } else {
                                *blocks::GRASS
                            };
                            chunk.set_block(local_x, height as i16, local_z, flower);
                        }
                    }
                    Biome::Plains | Biome::Forest | Biome::BirchForest => {
                        // Occasional grass with rare flowers
                        if veg_noise > 0.3 {
                            if type_noise > 0.6 {
                                let flower = if type_noise > 0.8 {
                                    *blocks::DANDELION
                                } else {
                                    *blocks::POPPY
                                };
                                chunk.set_block(local_x, height as i16, local_z, flower);
                            } else {
                                chunk.set_block(local_x, height as i16, local_z, *blocks::GRASS);
                            }
                        }
                    }
                    Biome::DarkForest => {
                        // Mushroom clusters
                        if veg_noise > 0.5 {
                            let mushroom = if type_noise > 0.0 {
                                *blocks::RED_MUSHROOM
                            } else {
                                *blocks::BROWN_MUSHROOM
                            };
                            chunk.set_block(local_x, height as i16, local_z, mushroom);
                        }
                    }
                    Biome::Swamp => {
                        // Lily pads in shallow water areas
                        if height == Self::SEA_LEVEL && veg_noise > 0.2 {
                            chunk.set_block(local_x, height as i16, local_z, *blocks::LILY_PAD);
                        }
                    }
                    Biome::Taiga | Biome::SnowyTaiga => {
                        // Ferns and occasional grass
                        if veg_noise > 0.4 {
                            chunk.set_block(local_x, height as i16, local_z, *blocks::GRASS);
                        }
                    }
                    Biome::Savanna => {
                        // Sparse grass
                        if veg_noise > 0.5 {
                            chunk.set_block(local_x, height as i16, local_z, *blocks::GRASS);
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    /// Add stone variants (granite, diorite, andesite) and deepslate underground.
    /// Uses SIMD batching to process 4 X positions at a time.
    fn add_stone_variants(&self, chunk: &mut Chunk, chunk_x: i32, chunk_z: i32) {
        for y in -60i16..80 {
            let fy = y as f64;
            let transition = if y < 0 {
                ((-y) as f64 / 8.0).min(1.0)
            } else {
                0.0
            };

            for local_z in 0u8..16 {
                let world_z = chunk_z * 16 + local_z as i32;
                let fz = world_z as f64;

                // Process 4 X positions at a time using SIMD
                for local_x_batch in (0u8..16).step_by(4) {
                    let fx = [
                        (chunk_x * 16 + local_x_batch as i32) as f64,
                        (chunk_x * 16 + local_x_batch as i32 + 1) as f64,
                        (chunk_x * 16 + local_x_batch as i32 + 2) as f64,
                        (chunk_x * 16 + local_x_batch as i32 + 3) as f64,
                    ];

                    // Check which positions have stone (we need to process)
                    let stones = [
                        chunk.get_block(local_x_batch, y, local_z) == *blocks::STONE,
                        chunk.get_block(local_x_batch + 1, y, local_z) == *blocks::STONE,
                        chunk.get_block(local_x_batch + 2, y, local_z) == *blocks::STONE,
                        chunk.get_block(local_x_batch + 3, y, local_z) == *blocks::STONE,
                    ];

                    // Skip if no stone blocks in this batch
                    if !stones[0] && !stones[1] && !stones[2] && !stones[3] {
                        continue;
                    }

                    // Batch sample noise for deepslate (only if y < 0)
                    let deepslate_noise = if y < 0 {
                        let x_scaled = [fx[0] * 0.1, fx[1] * 0.1, fx[2] * 0.1, fx[3] * 0.1];
                        let z_scaled = [fz * 0.1, fz * 0.1, fz * 0.1, fz * 0.1];
                        self.detail_noise.sample_4(x_scaled, fy * 0.1, z_scaled)
                    } else {
                        [0.0; 4]
                    };

                    // Batch sample variant noises
                    let x_v1 = [fx[0] * 0.05, fx[1] * 0.05, fx[2] * 0.05, fx[3] * 0.05];
                    let z_v1 = [fz * 0.05, fz * 0.05, fz * 0.05, fz * 0.05];
                    let variant1 = self.tree_noise.sample_4(x_v1, fy * 0.05, z_v1);

                    let x_v2 = [
                        fx[0] * 0.08 + 100.0,
                        fx[1] * 0.08 + 100.0,
                        fx[2] * 0.08 + 100.0,
                        fx[3] * 0.08 + 100.0,
                    ];
                    let z_v2 = [
                        fz * 0.08 + 100.0,
                        fz * 0.08 + 100.0,
                        fz * 0.08 + 100.0,
                        fz * 0.08 + 100.0,
                    ];
                    let variant2 = self.detail_noise.sample_4(x_v2, fy * 0.08, z_v2);

                    // Apply results for each position
                    for i in 0..4 {
                        if !stones[i] {
                            continue;
                        }

                        let local_x = local_x_batch + i as u8;

                        // Deepslate below Y=0
                        if y < 0 && deepslate_noise[i] < transition - 0.3 {
                            chunk.set_block(local_x, y, local_z, *blocks::DEEPSLATE);
                            continue;
                        }

                        let v1 = variant1[i];
                        let v2 = variant2[i];

                        // Granite blobs (more common in upper levels)
                        if v1 > 0.6 && v2 > 0.5 && y > -20 {
                            chunk.set_block(local_x, y, local_z, *blocks::GRANITE);
                        }
                        // Diorite blobs
                        else if v1 < -0.6 && v2 > 0.5 && y > -40 {
                            chunk.set_block(local_x, y, local_z, *blocks::DIORITE);
                        }
                        // Andesite blobs (more common in lower levels)
                        else if v2 < -0.6 && v1.abs() < 0.4 && y > -50 {
                            chunk.set_block(local_x, y, local_z, *blocks::ANDESITE);
                        }
                        // Tuff around Y=0
                        else if y < 10 && y > -20 && v1 > 0.5 && v2 < -0.3 {
                            chunk.set_block(local_x, y, local_z, *blocks::TUFF);
                        }
                    }
                }
            }
        }
    }

    /// Add ore veins using vanilla-like height distributions.
    /// Uses SIMD batching to process 4 X positions at a time.
    fn add_ores(&self, chunk: &mut Chunk, chunk_x: i32, chunk_z: i32) {
        // Use deterministic RNG for ore placement
        let ore_seed = self
            .seed
            .wrapping_add((chunk_x as i64).wrapping_mul(341873128712))
            .wrapping_add((chunk_z as i64).wrapping_mul(132897987541))
            .wrapping_mul(0xDEADBEEF);

        for y in -60i16..128 {
            let fy = y as f64;

            for local_z in 0u8..16 {
                let world_z = chunk_z * 16 + local_z as i32;
                let fz = world_z as f64;

                // Process 4 X positions at a time using SIMD
                for local_x_batch in (0u8..16).step_by(4) {
                    let world_x = [
                        chunk_x * 16 + local_x_batch as i32,
                        chunk_x * 16 + local_x_batch as i32 + 1,
                        chunk_x * 16 + local_x_batch as i32 + 2,
                        chunk_x * 16 + local_x_batch as i32 + 3,
                    ];
                    let fx = [
                        world_x[0] as f64,
                        world_x[1] as f64,
                        world_x[2] as f64,
                        world_x[3] as f64,
                    ];

                    // Check which positions have stone or deepslate
                    let blocks_at = [
                        chunk.get_block(local_x_batch, y, local_z),
                        chunk.get_block(local_x_batch + 1, y, local_z),
                        chunk.get_block(local_x_batch + 2, y, local_z),
                        chunk.get_block(local_x_batch + 3, y, local_z),
                    ];

                    let is_stone = [
                        blocks_at[0] == *blocks::STONE,
                        blocks_at[1] == *blocks::STONE,
                        blocks_at[2] == *blocks::STONE,
                        blocks_at[3] == *blocks::STONE,
                    ];
                    let is_deepslate = [
                        blocks_at[0] == *blocks::DEEPSLATE,
                        blocks_at[1] == *blocks::DEEPSLATE,
                        blocks_at[2] == *blocks::DEEPSLATE,
                        blocks_at[3] == *blocks::DEEPSLATE,
                    ];

                    let needs_processing = [
                        is_stone[0] || is_deepslate[0],
                        is_stone[1] || is_deepslate[1],
                        is_stone[2] || is_deepslate[2],
                        is_stone[3] || is_deepslate[3],
                    ];

                    // Skip if no stone/deepslate blocks in this batch
                    if !needs_processing[0]
                        && !needs_processing[1]
                        && !needs_processing[2]
                        && !needs_processing[3]
                    {
                        continue;
                    }

                    // Batch sample all 3 ore noises using SIMD
                    let x_n1 = [fx[0] * 0.15, fx[1] * 0.15, fx[2] * 0.15, fx[3] * 0.15];
                    let z_n1 = [fz * 0.15, fz * 0.15, fz * 0.15, fz * 0.15];
                    let ore_noise1 = self.detail_noise.sample_4(x_n1, fy * 0.15, z_n1);

                    let x_n2 = [
                        fx[0] * 0.2 + 50.0,
                        fx[1] * 0.2 + 50.0,
                        fx[2] * 0.2 + 50.0,
                        fx[3] * 0.2 + 50.0,
                    ];
                    let z_n2 = [
                        fz * 0.2 + 50.0,
                        fz * 0.2 + 50.0,
                        fz * 0.2 + 50.0,
                        fz * 0.2 + 50.0,
                    ];
                    let ore_noise2 = self.tree_noise.sample_4(x_n2, fy * 0.2, z_n2);

                    let x_n3 = [
                        fx[0] * 0.12 + 100.0,
                        fx[1] * 0.12 + 100.0,
                        fx[2] * 0.12 + 100.0,
                        fx[3] * 0.12 + 100.0,
                    ];
                    let z_n3 = [fz * 0.12, fz * 0.12, fz * 0.12, fz * 0.12];
                    let ore_noise3 = self.tree_noise.sample_4(x_n3, fy * 0.12 + 100.0, z_n3);

                    // Apply results for each position
                    for i in 0..4 {
                        if !needs_processing[i] {
                            continue;
                        }

                        let local_x = local_x_batch + i as u8;
                        let n1 = ore_noise1[i];
                        let n2 = ore_noise2[i];
                        let n3 = ore_noise3[i];

                        // Position-based hash for variety
                        let hash = ((world_x[i].wrapping_mul(1337)
                            ^ world_z.wrapping_mul(7919)
                            ^ (y as i32).wrapping_mul(13))
                            as u32)
                            ^ (ore_seed as u32);
                        let hash_f = (hash % 1000) as f64 / 1000.0;

                        let ore = if y >= 5 && y <= 128 && n1 > 0.75 - (y as f64 / 300.0) {
                            // Coal ore
                            if is_deepslate[i] {
                                *blocks::DEEPSLATE_COAL_ORE
                            } else {
                                *blocks::COAL_ORE
                            }
                        } else if y >= -60 && y <= 64 && n2 > 0.78 {
                            // Iron ore
                            if is_deepslate[i] {
                                *blocks::DEEPSLATE_IRON_ORE
                            } else {
                                *blocks::IRON_ORE
                            }
                        } else if y >= -16 && y <= 112 && n1 < -0.78 && n3 > 0.3 {
                            // Copper ore
                            if is_deepslate[i] {
                                *blocks::DEEPSLATE_COPPER_ORE
                            } else {
                                *blocks::COPPER_ORE
                            }
                        } else if y >= -60 && y <= 32 && n3 > 0.85 {
                            // Gold ore
                            if is_deepslate[i] {
                                *blocks::DEEPSLATE_GOLD_ORE
                            } else {
                                *blocks::GOLD_ORE
                            }
                        } else if y >= -60 && y <= 16 && n2 < -0.78 {
                            // Redstone ore
                            if is_deepslate[i] {
                                *blocks::DEEPSLATE_REDSTONE_ORE
                            } else {
                                *blocks::REDSTONE_ORE
                            }
                        } else if y >= -60 && y <= 64 && n3 < -0.88 && hash_f > 0.7 {
                            // Lapis ore
                            if is_deepslate[i] {
                                *blocks::DEEPSLATE_LAPIS_ORE
                            } else {
                                *blocks::LAPIS_ORE
                            }
                        } else if y >= -60 && y <= 16 && n1 > 0.92 && n2 > 0.5 {
                            // Diamond ore
                            if is_deepslate[i] {
                                *blocks::DEEPSLATE_DIAMOND_ORE
                            } else {
                                *blocks::DIAMOND_ORE
                            }
                        } else if y >= -16 && y <= 100 && n1 > 0.95 && n2 < -0.5 && n3 > 0.7 {
                            // Emerald ore (mountains only)
                            let biome = self.get_biome(world_x[i], world_z);
                            if matches!(biome, Biome::WindsweptHills | Biome::SnowyTaiga) {
                                if is_deepslate[i] {
                                    *blocks::DEEPSLATE_EMERALD_ORE
                                } else {
                                    *blocks::EMERALD_ORE
                                }
                            } else {
                                continue;
                            }
                        } else {
                            continue;
                        };

                        chunk.set_block(local_x, y, local_z, ore);
                    }
                }
            }
        }
    }

    /// Carve caves into the chunk using vanilla worm algorithm.
    /// Ported from MapGenCaves.java
    fn carve_caves(&self, chunk: &mut Chunk, chunk_x: i32, chunk_z: i32) {
        use std::f64::consts::PI;

        // Check nearby chunks for cave starts that might reach into this chunk
        let range = 8; // Cave range in chunks
        for cx in (chunk_x - range)..=(chunk_x + range) {
            for cz in (chunk_z - range)..=(chunk_z + range) {
                // Seed RNG for this chunk
                let chunk_seed = self
                    .seed
                    .wrapping_add((cx as i64).wrapping_mul(341873128712))
                    .wrapping_add((cz as i64).wrapping_mul(132897987541));
                let mut rng = JavaRandom::from_seed(chunk_seed);

                // Number of cave starts = rand(rand(rand(15)+1)+1)
                // But only 1 in 7 chunks have caves
                let r1 = rng.next_int(15) + 1;
                let r2 = rng.next_int(r1 as u32) + 1;
                let mut cave_count = rng.next_int(r2 as u32);
                if rng.next_int(7) != 0 {
                    cave_count = 0;
                }

                for _ in 0..cave_count {
                    // Start position
                    let start_x = (cx * 16 + rng.next_int(16)) as f64;
                    let y_bound = rng.next_int(120) + 8;
                    let start_y = rng.next_int(y_bound.max(1) as u32) as f64;
                    let start_z = (cz * 16 + rng.next_int(16)) as f64;

                    // Number of branches
                    let mut branches = 1;
                    if rng.next_int(4) == 0 {
                        // Large room
                        self.carve_cave_room(
                            chunk, chunk_x, chunk_z, &mut rng, start_x, start_y, start_z,
                        );
                        branches += rng.next_int(4);
                    }

                    for _ in 0..branches {
                        let yaw = rng.next_float() * PI as f32 * 2.0;
                        let pitch = (rng.next_float() - 0.5) * 2.0 / 8.0;
                        let mut width = rng.next_float() * 2.0 + rng.next_float();

                        if rng.next_int(10) == 0 {
                            width *= rng.next_float() * rng.next_float() * 3.0 + 1.0;
                        }

                        self.carve_cave_tunnel(
                            chunk,
                            chunk_x,
                            chunk_z,
                            rng.next_long(),
                            start_x,
                            start_y,
                            start_z,
                            width,
                            yaw,
                            pitch,
                            0,
                            0,
                            1.0,
                        );
                    }
                }
            }
        }
    }

    /// Carve a large cave room.
    fn carve_cave_room(
        &self,
        chunk: &mut Chunk,
        chunk_x: i32,
        chunk_z: i32,
        rng: &mut JavaRandom,
        x: f64,
        y: f64,
        z: f64,
    ) {
        let width = 1.0 + rng.next_float() * 6.0;
        self.carve_cave_tunnel(
            chunk,
            chunk_x,
            chunk_z,
            rng.next_long(),
            x,
            y,
            z,
            width,
            0.0,
            0.0,
            -1,
            -1,
            0.5,
        );
    }

    /// Carve a cave tunnel (worm algorithm from vanilla).
    fn carve_cave_tunnel(
        &self,
        chunk: &mut Chunk,
        chunk_x: i32,
        chunk_z: i32,
        seed: i64,
        mut x: f64,
        mut y: f64,
        mut z: f64,
        width: f32,
        mut yaw: f32,
        mut pitch: f32,
        start_idx: i32,
        end_idx: i32,
        height_ratio: f64,
    ) {
        use std::f64::consts::PI;

        let center_x = (chunk_x * 16 + 8) as f64;
        let center_z = (chunk_z * 16 + 8) as f64;

        let mut yaw_change = 0.0f32;
        let mut pitch_change = 0.0f32;

        let mut rng = JavaRandom::from_seed(seed);

        let range: i32 = 8 * 16 - 16;
        let mut end_idx = end_idx;
        if end_idx <= 0 {
            end_idx = range - rng.next_int((range / 4) as u32);
        }

        let mut start_idx = start_idx;
        let is_room = start_idx == -1;
        if is_room {
            start_idx = end_idx / 2;
        }

        let branch_point = rng.next_int((end_idx / 2).max(1) as u32) + end_idx / 4;
        let steep_tunnel = rng.next_int(6) == 0;

        for i in start_idx..end_idx {
            // Cave size varies with sin function
            let radius = 1.5 + (((i as f64) * PI / (end_idx as f64)).sin() * width as f64);
            let v_radius = radius * height_ratio;

            // Move in direction
            let cos_pitch = pitch.cos();
            let sin_pitch = pitch.sin();
            x += (yaw.cos() * cos_pitch) as f64;
            y += sin_pitch as f64;
            z += (yaw.sin() * cos_pitch) as f64;

            // Pitch changes
            if steep_tunnel {
                pitch *= 0.92;
            } else {
                pitch *= 0.7;
            }

            pitch += pitch_change * 0.1;
            yaw += yaw_change * 0.1;

            pitch_change *= 0.9;
            yaw_change *= 0.75;

            pitch_change += (rng.next_float() - rng.next_float()) * rng.next_float() * 2.0;
            yaw_change += (rng.next_float() - rng.next_float()) * rng.next_float() * 4.0;

            // Branch at midpoint
            if !is_room && i == branch_point && width > 1.0 && end_idx > 0 {
                self.carve_cave_tunnel(
                    chunk,
                    chunk_x,
                    chunk_z,
                    rng.next_long(),
                    x,
                    y,
                    z,
                    rng.next_float() * 0.5 + 0.5,
                    yaw - PI as f32 / 2.0,
                    pitch / 3.0,
                    i,
                    end_idx,
                    1.0,
                );
                self.carve_cave_tunnel(
                    chunk,
                    chunk_x,
                    chunk_z,
                    rng.next_long(),
                    x,
                    y,
                    z,
                    rng.next_float() * 0.5 + 0.5,
                    yaw + PI as f32 / 2.0,
                    pitch / 3.0,
                    i,
                    end_idx,
                    1.0,
                );
                return;
            }

            // Skip some positions randomly
            if is_room || rng.next_int(4) != 0 {
                // Check if in range of target chunk
                let dx = x - center_x;
                let dz = z - center_z;
                let remaining = (end_idx - i) as f64;
                let check_rad = (width + 2.0 + 16.0) as f64;

                if dx * dx + dz * dz - remaining * remaining > check_rad * check_rad {
                    return;
                }

                // Check if we should carve in this chunk
                if x >= center_x - 16.0 - radius * 2.0
                    && z >= center_z - 16.0 - radius * 2.0
                    && x <= center_x + 16.0 + radius * 2.0
                    && z <= center_z + 16.0 + radius * 2.0
                {
                    // Carve blocks in ellipsoid
                    let min_x = ((x - radius).floor() as i32 - chunk_x * 16).max(0);
                    let max_x = ((x + radius).floor() as i32 - chunk_x * 16 + 1).min(16);
                    let min_y = ((y - v_radius).floor() as i32).max(1);
                    let max_y = ((y + v_radius).floor() as i32 + 1).min(248);
                    let min_z = ((z - radius).floor() as i32 - chunk_z * 16).max(0);
                    let max_z = ((z + radius).floor() as i32 - chunk_z * 16 + 1).min(16);

                    for lx in min_x..max_x {
                        let dx = ((lx + chunk_x * 16) as f64 + 0.5 - x) / radius;

                        for lz in min_z..max_z {
                            let dz = ((lz + chunk_z * 16) as f64 + 0.5 - z) / radius;

                            if dx * dx + dz * dz < 1.0 {
                                for ly in (min_y..max_y).rev() {
                                    let dy = ((ly - 1) as f64 + 0.5 - y) / v_radius;

                                    if dy > -0.7 && dx * dx + dy * dy + dz * dz < 1.0 {
                                        let current =
                                            chunk.get_block(lx as u8, ly as i16, lz as u8);
                                        // Only carve stone, dirt, grass - not water
                                        if current != *blocks::WATER && current != *blocks::BEDROCK
                                        {
                                            // Lava below Y=10
                                            if ly < 10 {
                                                chunk.set_block(
                                                    lx as u8,
                                                    ly as i16,
                                                    lz as u8,
                                                    *blocks::LAVA,
                                                );
                                            } else {
                                                chunk.set_block(
                                                    lx as u8,
                                                    ly as i16,
                                                    lz as u8,
                                                    *blocks::AIR,
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    if is_room {
                        break;
                    }
                }
            }
        }
    }

    /// Carve ravines (vanilla-accurate from MapGenRavine.java)
    /// Ravines are rarer but larger than caves, with a distinctive tall/narrow shape
    fn carve_ravines(&self, chunk: &mut Chunk, chunk_x: i32, chunk_z: i32) {
        use std::f64::consts::PI;

        // Check nearby chunks for ravine starts
        let range = 8;
        for cx in (chunk_x - range)..=(chunk_x + range) {
            for cz in (chunk_z - range)..=(chunk_z + range) {
                // Seed RNG for this chunk
                let chunk_seed = self
                    .seed
                    .wrapping_add((cx as i64).wrapping_mul(341873128712))
                    .wrapping_add((cz as i64).wrapping_mul(132897987541))
                    .wrapping_mul(0x12345678);
                let mut rng = JavaRandom::from_seed(chunk_seed);

                // Ravines are rare: 1 in 50 chunks
                if rng.next_int(50) != 0 {
                    continue;
                }

                // Start position - ravines start higher than caves (Y 20-60)
                let start_x = (cx * 16 + rng.next_int(16)) as f64;
                let y_bound = rng.next_int(40) + 8;
                let y_range = rng.next_int(y_bound.max(1) as u32) + 20;
                let start_y = y_range as f64;
                let start_z = (cz * 16 + rng.next_int(16)) as f64;

                let yaw = rng.next_float() * PI as f32 * 2.0;
                let pitch = (rng.next_float() - 0.5) * 2.0 / 8.0;
                // Ravines are wider than caves
                let width = (rng.next_float() * 2.0 + rng.next_float()) * 2.0;

                self.carve_ravine_tunnel(
                    chunk,
                    chunk_x,
                    chunk_z,
                    rng.next_long(),
                    start_x,
                    start_y,
                    start_z,
                    width,
                    yaw,
                    pitch,
                    0,
                    0,
                    3.0, // Height ratio of 3 makes ravines tall/narrow
                );
            }
        }
    }

    /// Carve a ravine tunnel (similar to cave but taller and shallower)
    fn carve_ravine_tunnel(
        &self,
        chunk: &mut Chunk,
        chunk_x: i32,
        chunk_z: i32,
        seed: i64,
        mut x: f64,
        mut y: f64,
        mut z: f64,
        width: f32,
        mut yaw: f32,
        mut pitch: f32,
        start_idx: i32,
        end_idx: i32,
        height_ratio: f64,
    ) {
        use std::f64::consts::PI;

        let center_x = (chunk_x * 16 + 8) as f64;
        let center_z = (chunk_z * 16 + 8) as f64;

        let mut yaw_change = 0.0f32;
        let mut pitch_change = 0.0f32;

        let mut rng = JavaRandom::from_seed(seed);

        let range: i32 = 8 * 16 - 16;
        let mut end_idx = end_idx;
        if end_idx <= 0 {
            end_idx = range - rng.next_int((range / 4) as u32);
        }

        let mut start_idx = start_idx;
        let is_room = start_idx == -1;
        if is_room {
            start_idx = end_idx / 2;
        }

        for i in start_idx..end_idx {
            // Ravine size - wider than caves
            let radius = 1.5 + (((i as f64) * PI / (end_idx as f64)).sin() * width as f64);
            // Random width variation per step
            let width_mult = rng.next_float() * 0.25 + 0.75;
            let h_radius = radius * width_mult as f64;
            let v_radius = radius * height_ratio * width_mult as f64;

            // Move in direction (ravines move more horizontally)
            let cos_pitch = pitch.cos();
            let sin_pitch = pitch.sin();
            x += (yaw.cos() * cos_pitch) as f64;
            y += (sin_pitch * 0.3) as f64; // Less vertical movement
            z += (yaw.sin() * cos_pitch) as f64;

            // Direction changes (ravines curve less than caves)
            pitch *= 0.7;
            pitch += pitch_change * 0.05;
            yaw += yaw_change * 0.05;

            pitch_change *= 0.8;
            yaw_change *= 0.5;

            pitch_change += (rng.next_float() - rng.next_float()) * rng.next_float() * 2.0;
            yaw_change += (rng.next_float() - rng.next_float()) * rng.next_float() * 4.0;

            if is_room || rng.next_int(4) != 0 {
                let dx = x - center_x;
                let dz = z - center_z;
                let remaining = (end_idx - i) as f64;
                let check_rad = (width + 2.0 + 16.0) as f64;

                if dx * dx + dz * dz - remaining * remaining > check_rad * check_rad {
                    return;
                }

                if x >= center_x - 16.0 - h_radius * 2.0
                    && z >= center_z - 16.0 - h_radius * 2.0
                    && x <= center_x + 16.0 + h_radius * 2.0
                    && z <= center_z + 16.0 + h_radius * 2.0
                {
                    let min_x = ((x - h_radius).floor() as i32 - chunk_x * 16).max(0);
                    let max_x = ((x + h_radius).floor() as i32 - chunk_x * 16 + 1).min(16);
                    let min_y = ((y - v_radius).floor() as i32).max(1);
                    let max_y = ((y + v_radius).floor() as i32 + 1).min(248);
                    let min_z = ((z - h_radius).floor() as i32 - chunk_z * 16).max(0);
                    let max_z = ((z + h_radius).floor() as i32 - chunk_z * 16 + 1).min(16);

                    for lx in min_x..max_x {
                        let dx = ((lx + chunk_x * 16) as f64 + 0.5 - x) / h_radius;

                        for lz in min_z..max_z {
                            let dz = ((lz + chunk_z * 16) as f64 + 0.5 - z) / h_radius;

                            if dx * dx + dz * dz < 1.0 {
                                for ly in (min_y..max_y).rev() {
                                    let dy = ((ly - 1) as f64 + 0.5 - y) / v_radius;

                                    // Ravine shape: wider at bottom (inverted from caves)
                                    if dx * dx + dz * dz + (dy * dy) / 6.0 < 1.0 {
                                        let current =
                                            chunk.get_block(lx as u8, ly as i16, lz as u8);
                                        if current != *blocks::WATER && current != *blocks::BEDROCK
                                        {
                                            if ly < 10 {
                                                chunk.set_block(
                                                    lx as u8,
                                                    ly as i16,
                                                    lz as u8,
                                                    *blocks::LAVA,
                                                );
                                            } else {
                                                chunk.set_block(
                                                    lx as u8,
                                                    ly as i16,
                                                    lz as u8,
                                                    *blocks::AIR,
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    if is_room {
                        break;
                    }
                }
            }
        }
    }

    fn has_structure_in_chunk(&self, chunk_x: i32, chunk_z: i32) -> bool {
        use super::density::SinglePointContext;

        let config = StructureConfig::get(StructureType::Village);
        let reg_x = chunk_x.div_euclid(config.region_size);
        let reg_z = chunk_z.div_euclid(config.region_size);
        let pos = get_structure_pos(&config, self.seed, reg_x, reg_z);
        if pos.chunk_x == chunk_x && pos.chunk_z == chunk_z {
            // Sample density to find approximate surface height
            for y in (Self::SEA_LEVEL + 1..=90).rev() {
                let ctx = SinglePointContext::new(pos.x, y, pos.z);
                if self.router.final_density.compute(&ctx) > 0.0 {
                    return true; // Found solid ground in valid range
                }
            }
        }
        false
    }

    fn add_structures(&self, chunk: &mut Chunk, chunk_x: i32, chunk_z: i32) {
        // Check for village
        let village_config = StructureConfig::get(StructureType::Village);
        let v_reg_x = chunk_x.div_euclid(village_config.region_size);
        let v_reg_z = chunk_z.div_euclid(village_config.region_size);
        let v_pos = get_structure_pos(&village_config, self.seed, v_reg_x, v_reg_z);

        if v_pos.chunk_x == chunk_x && v_pos.chunk_z == chunk_z {
            let local_x = (v_pos.x & 15) as u8;
            let local_z = (v_pos.z & 15) as u8;
            let height = self.find_surface_height(chunk, local_x, local_z);
            let biome = self.get_biome(v_pos.x, v_pos.z);
            // Villages in plains, savanna, taiga, desert
            if height > Self::SEA_LEVEL
                && height < 90
                && matches!(
                    biome,
                    Biome::Plains | Biome::Savanna | Biome::Taiga | Biome::Desert
                )
            {
                self.place_village_well(chunk, local_x, height as i16, local_z);
            }
        }

        // Check for desert pyramid
        let pyramid_config = StructureConfig::get(StructureType::DesertPyramid);
        let p_reg_x = chunk_x.div_euclid(pyramid_config.region_size);
        let p_reg_z = chunk_z.div_euclid(pyramid_config.region_size);
        let p_pos = get_structure_pos(&pyramid_config, self.seed, p_reg_x, p_reg_z);

        if p_pos.chunk_x == chunk_x && p_pos.chunk_z == chunk_z {
            let local_x = (p_pos.x & 15) as u8;
            let local_z = (p_pos.z & 15) as u8;
            let height = self.find_surface_height(chunk, local_x, local_z);
            let biome = self.get_biome(p_pos.x, p_pos.z);
            if height > Self::SEA_LEVEL && biome == Biome::Desert {
                self.place_desert_pyramid(chunk, local_x, height as i16, local_z);
            }
        }

        // Check for swamp hut
        let hut_config = StructureConfig::get(StructureType::SwampHut);
        let h_reg_x = chunk_x.div_euclid(hut_config.region_size);
        let h_reg_z = chunk_z.div_euclid(hut_config.region_size);
        let h_pos = get_structure_pos(&hut_config, self.seed, h_reg_x, h_reg_z);

        if h_pos.chunk_x == chunk_x && h_pos.chunk_z == chunk_z {
            let local_x = (h_pos.x & 15) as u8;
            let local_z = (h_pos.z & 15) as u8;
            let height = self.find_surface_height(chunk, local_x, local_z);
            let biome = self.get_biome(h_pos.x, h_pos.z);
            if biome == Biome::Swamp && height >= Self::SEA_LEVEL {
                self.place_swamp_hut(chunk, local_x, height as i16, local_z);
            }
        }

        // Check for igloo
        let igloo_config = StructureConfig::get(StructureType::Igloo);
        let i_reg_x = chunk_x.div_euclid(igloo_config.region_size);
        let i_reg_z = chunk_z.div_euclid(igloo_config.region_size);
        let i_pos = get_structure_pos(&igloo_config, self.seed, i_reg_x, i_reg_z);

        if i_pos.chunk_x == chunk_x && i_pos.chunk_z == chunk_z {
            let local_x = (i_pos.x & 15) as u8;
            let local_z = (i_pos.z & 15) as u8;
            let height = self.find_surface_height(chunk, local_x, local_z);
            let biome = self.get_biome(i_pos.x, i_pos.z);
            if matches!(biome, Biome::SnowyTaiga | Biome::SnowyMountains)
                && height > Self::SEA_LEVEL
            {
                self.place_igloo(chunk, local_x, height as i16, local_z);
            }
        }

        // Check for jungle temple
        let jungle_config = StructureConfig::get(StructureType::JungleTemple);
        let j_reg_x = chunk_x.div_euclid(jungle_config.region_size);
        let j_reg_z = chunk_z.div_euclid(jungle_config.region_size);
        let j_pos = get_structure_pos(&jungle_config, self.seed, j_reg_x, j_reg_z);

        if j_pos.chunk_x == chunk_x && j_pos.chunk_z == chunk_z {
            let local_x = (j_pos.x & 15) as u8;
            let local_z = (j_pos.z & 15) as u8;
            let height = self.find_surface_height(chunk, local_x, local_z);
            let biome = self.get_biome(j_pos.x, j_pos.z);
            if biome == Biome::Jungle && height > Self::SEA_LEVEL {
                self.place_jungle_temple(chunk, local_x, height as i16, local_z);
            }
        }
    }

    fn place_village_well(&self, chunk: &mut Chunk, cx: u8, ground_y: i16, cz: u8) {
        if cx < 2 || cx > 13 || cz < 2 || cz > 13 {
            return;
        }
        for dx in -2i8..=2 {
            for dz in -2i8..=2 {
                let x = (cx as i8 + dx) as u8;
                let z = (cz as i8 + dz) as u8;
                chunk.set_block(x, ground_y - 1, z, *blocks::COBBLESTONE);
                chunk.set_block(x, ground_y, z, *blocks::COBBLESTONE);
            }
        }
        for y_off in 1i16..=3 {
            for dx in -2i8..=2 {
                for dz in -2i8..=2 {
                    let x = (cx as i8 + dx) as u8;
                    let z = (cz as i8 + dz) as u8;
                    if dx.abs() == 2 || dz.abs() == 2 {
                        chunk.set_block(x, ground_y + y_off, z, *blocks::COBBLESTONE);
                    }
                }
            }
        }
        for dx in -2i8..=2 {
            for dz in -2i8..=2 {
                let x = (cx as i8 + dx) as u8;
                let z = (cz as i8 + dz) as u8;
                chunk.set_block(x, ground_y + 4, z, *blocks::COBBLESTONE);
            }
        }
        for dx in -1i8..=1 {
            for dz in -1i8..=1 {
                let x = (cx as i8 + dx) as u8;
                let z = (cz as i8 + dz) as u8;
                chunk.set_block(x, ground_y, z, *blocks::WATER);
                for y_off in 1i16..=3 {
                    chunk.set_block(x, ground_y + y_off, z, *blocks::AIR);
                }
            }
        }
    }

    /// Place a simple desert pyramid structure.
    fn place_desert_pyramid(&self, chunk: &mut Chunk, cx: u8, ground_y: i16, cz: u8) {
        if cx < 4 || cx > 11 || cz < 4 || cz > 11 {
            return;
        }

        // Base platform
        for dx in -4i8..=4 {
            for dz in -4i8..=4 {
                let x = (cx as i8 + dx) as u8;
                let z = (cz as i8 + dz) as u8;
                chunk.set_block(x, ground_y - 1, z, *blocks::SANDSTONE);
                chunk.set_block(x, ground_y, z, *blocks::SANDSTONE);
            }
        }

        // Stepped pyramid (4 levels)
        for level in 0i16..4 {
            let radius = 4 - level as i8;
            for dx in -radius..=radius {
                for dz in -radius..=radius {
                    let x = (cx as i8 + dx) as u8;
                    let z = (cz as i8 + dz) as u8;
                    chunk.set_block(x, ground_y + 1 + level, z, *blocks::SANDSTONE);
                }
            }
        }

        // Hollow out interior
        for level in 1i16..3 {
            for dx in -2i8..=2 {
                for dz in -2i8..=2 {
                    let x = (cx as i8 + dx) as u8;
                    let z = (cz as i8 + dz) as u8;
                    chunk.set_block(x, ground_y + level, z, *blocks::AIR);
                }
            }
        }

        // Entrance
        chunk.set_block(cx, ground_y + 1, cz.saturating_sub(3), *blocks::AIR);
        chunk.set_block(cx, ground_y + 2, cz.saturating_sub(3), *blocks::AIR);
    }

    /// Place a witch hut structure (swamp).
    fn place_swamp_hut(&self, chunk: &mut Chunk, cx: u8, ground_y: i16, cz: u8) {
        if cx < 3 || cx > 12 || cz < 3 || cz > 12 {
            return;
        }

        // Stilts (oak logs)
        for dy in 0i16..3 {
            chunk.set_block(cx - 2, ground_y + dy, cz - 2, *blocks::OAK_LOG);
            chunk.set_block(cx + 2, ground_y + dy, cz - 2, *blocks::OAK_LOG);
            chunk.set_block(cx - 2, ground_y + dy, cz + 2, *blocks::OAK_LOG);
            chunk.set_block(cx + 2, ground_y + dy, cz + 2, *blocks::OAK_LOG);
        }

        // Floor (spruce planks)
        for dx in -2i8..=2 {
            for dz in -2i8..=2 {
                let x = (cx as i8 + dx) as u8;
                let z = (cz as i8 + dz) as u8;
                chunk.set_block(x, ground_y + 3, z, *blocks::SPRUCE_PLANKS);
            }
        }

        // Walls
        for dy in 4i16..7 {
            for dx in [-2i8, 2].iter() {
                for dz in -2i8..=2 {
                    let x = (cx as i8 + dx) as u8;
                    let z = (cz as i8 + dz) as u8;
                    chunk.set_block(x, ground_y + dy, z, *blocks::SPRUCE_PLANKS);
                }
            }
            for dz in [-2i8, 2].iter() {
                for dx in -1i8..=1 {
                    let x = (cx as i8 + dx) as u8;
                    let z = (cz as i8 + dz) as u8;
                    chunk.set_block(x, ground_y + dy, z, *blocks::SPRUCE_PLANKS);
                }
            }
        }

        // Roof (spruce stairs pattern - simplified flat)
        for dx in -3i8..=3 {
            for dz in -3i8..=3 {
                let x = (cx as i8 + dx) as u8;
                let z = (cz as i8 + dz) as u8;
                if x < 16 && z < 16 {
                    chunk.set_block(x, ground_y + 7, z, *blocks::SPRUCE_PLANKS);
                }
            }
        }

        // Interior air
        for dy in 4i16..7 {
            for dx in -1i8..=1 {
                for dz in -1i8..=1 {
                    let x = (cx as i8 + dx) as u8;
                    let z = (cz as i8 + dz) as u8;
                    chunk.set_block(x, ground_y + dy, z, *blocks::AIR);
                }
            }
        }
    }

    /// Place an igloo structure (snowy biomes).
    fn place_igloo(&self, chunk: &mut Chunk, cx: u8, ground_y: i16, cz: u8) {
        if cx < 3 || cx > 12 || cz < 3 || cz > 12 {
            return;
        }

        // Simple dome shape with snow blocks
        for dy in 0i16..3 {
            let radius = 3 - dy as i8;
            for dx in -radius..=radius {
                for dz in -radius..=radius {
                    let dist_sq = dx * dx + dz * dz;
                    if dist_sq <= (radius * radius) as i64 as i8 {
                        let x = (cx as i8 + dx) as u8;
                        let z = (cz as i8 + dz) as u8;
                        // Shell only (hollow inside)
                        let is_edge =
                            dist_sq >= ((radius - 1) * (radius - 1).max(0)) as i64 as i8 || dy == 0;
                        if is_edge {
                            chunk.set_block(x, ground_y + dy, z, *blocks::SNOW_BLOCK);
                        } else {
                            chunk.set_block(x, ground_y + dy, z, *blocks::AIR);
                        }
                    }
                }
            }
        }
        // Top cap
        chunk.set_block(cx, ground_y + 3, cz, *blocks::SNOW_BLOCK);

        // Entrance
        chunk.set_block(cx, ground_y, cz.saturating_sub(3), *blocks::AIR);
        chunk.set_block(cx, ground_y + 1, cz.saturating_sub(3), *blocks::AIR);
    }

    /// Place a jungle temple structure.
    fn place_jungle_temple(&self, chunk: &mut Chunk, cx: u8, ground_y: i16, cz: u8) {
        if cx < 4 || cx > 11 || cz < 4 || cz > 11 {
            return;
        }

        // Base (mossy cobblestone)
        for dy in 0i16..6 {
            for dx in -3i8..=3 {
                for dz in -3i8..=3 {
                    let x = (cx as i8 + dx) as u8;
                    let z = (cz as i8 + dz) as u8;
                    let is_wall = dx.abs() == 3 || dz.abs() == 3;
                    if dy == 0 || is_wall {
                        chunk.set_block(x, ground_y + dy, z, *blocks::COBBLESTONE);
                    } else if dy < 5 {
                        chunk.set_block(x, ground_y + dy, z, *blocks::AIR);
                    }
                }
            }
        }

        // Stepped roof
        for level in 0i16..2 {
            let radius = (2 - level) as i8;
            for dx in -radius..=radius {
                for dz in -radius..=radius {
                    let x = (cx as i8 + dx) as u8;
                    let z = (cz as i8 + dz) as u8;
                    chunk.set_block(x, ground_y + 6 + level, z, *blocks::COBBLESTONE);
                }
            }
        }

        // Columns with vines
        for dy in 0i16..6 {
            chunk.set_block(cx - 2, ground_y + dy, cz - 2, *blocks::COBBLESTONE);
            chunk.set_block(cx + 2, ground_y + dy, cz - 2, *blocks::COBBLESTONE);
            chunk.set_block(cx - 2, ground_y + dy, cz + 2, *blocks::COBBLESTONE);
            chunk.set_block(cx + 2, ground_y + dy, cz + 2, *blocks::COBBLESTONE);
        }

        // Add vines
        for dy in 1i16..5 {
            chunk.set_block(cx - 3, ground_y + dy, cz, *blocks::VINE);
            chunk.set_block(cx + 3, ground_y + dy, cz, *blocks::VINE);
        }

        // Entrance
        for dy in 1i16..4 {
            chunk.set_block(cx, ground_y + dy, cz - 3, *blocks::AIR);
        }
    }
}
