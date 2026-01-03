//! 3D terrain generation using density functions, aquifers, and surface rules.
//!
//! This module implements Java Edition-style terrain generation with:
//! - Density functions for 3D terrain shaping (overhangs, caves, etc.)
//! - Cell-based interpolation for performance
//! - Aquifer system for underground water/lava pockets
//! - Surface rules for biome-based block placement

use super::constants::Biome;
use super::density::{
    CachingNoiseChunk, FunctionContext, NoiseRegistry,
    FlatCacheGrid, ColumnContext, ColumnContextGrid, compute_final_density,
};
use super::aquifer::NoiseBasedAquifer;
use super::ore_veinifier::OreVeinifier;
use super::surface::SurfaceSystem;
use unastar_noise::build_vanilla_surface_rule;
use super::xoroshiro::{JavaRandom, PositionalRandomFactory};
use crate::world::chunk::{blocks, Chunk};
use crate::world::generator::BiomeNoise;
use std::simd::prelude::*;

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
    /// Noise registry with instantiated noises from seed
    noises: NoiseRegistry,
    /// Surface rules system for biome-based surface blocks
    surface_system: SurfaceSystem,
    /// Positional random factory for ore vein generation.
    /// Created via `PositionalRandomFactory::fork_ore_random()`.
    ore_random: PositionalRandomFactory,
}

impl VanillaGenerator {
    /// Sea level
    pub const SEA_LEVEL: i32 = 63;

    /// Create a new vanilla generator with the given seed.
    pub fn new(seed: i64) -> Self {
        let biome_noise = BiomeNoise::from_seed(seed);

        // Create noise registry with all noises instantiated from seed
        let noises = NoiseRegistry::new(seed);

        // Build surface rules system using generated vanilla rules from JSON
        let surface_rule = build_vanilla_surface_rule(seed);
        let surface_system = SurfaceSystem::new(seed, surface_rule, biome_noise.clone());

        // Create positional random factory for ore vein generation
        // Java: this.oreRandom = this.random.fromHashOf("minecraft:ore").forkPositional()
        let base_random = PositionalRandomFactory::new(seed);
        let ore_random = base_random.fork_ore_random();

        Self {
            seed,
            biome_noise,
            noises,
            surface_system,
            ore_random,
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
        // Cache grids per chunk to avoid recreating them
        let mut grid_cache: std::collections::HashMap<(i32, i32), FlatCacheGrid> = std::collections::HashMap::new();

        for radius in 0i32..64 {
            for dx in -radius..=radius {
                for dz in -radius..=radius {
                    if dx.abs() != radius && dz.abs() != radius {
                        continue;
                    }

                    // Determine which chunk this position is in
                    let chunk_x = dx.div_euclid(16);
                    let chunk_z = dz.div_euclid(16);

                    // Get or create FlatCacheGrid for this chunk
                    let grid = grid_cache.entry((chunk_x, chunk_z)).or_insert_with(|| {
                        FlatCacheGrid::new(chunk_x, chunk_z, &self.noises)
                    });

                    // Find surface by scanning down from max height
                    let col = ColumnContext::new(dx, dz, &self.noises, grid);
                    for y in (Self::SEA_LEVEL + 1..=128).rev() {
                        let ctx = FunctionContext::new(dx, y, dz);
                        let density = compute_final_density(&ctx, &self.noises, grid, &col);

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

    /// Generate a chunk using 3D density functions with cell-based caching.
    ///
    /// This is the Java Edition-style terrain generation using density functions
    /// and cell-based interpolation. It produces 3D features like overhangs and caves.
    ///
    /// The generation process:
    /// 1. Create FlatCacheGrid for Y-independent values (AOT compiled)
    /// 2. Create NoiseBasedAquifer for underground fluid handling
    /// 3. Create a CachingNoiseChunk for cell-based caching and interpolation
    /// 4. Initialize interpolators with first X slice using AOT compiled functions
    /// 5. Traverse cells (4x8x4 blocks) in X-outer, Z-middle, Y-inner order
    /// 6. Use trilinear interpolation for density (compute at 8 corners, interpolate interior)
    /// 7. Use aquifer system to determine fluid placement when density <= 0
    /// 8. Apply surface rules for biome-specific blocks
    pub fn generate_chunk(&self, chunk_x: i32, chunk_z: i32) -> Chunk {
        use super::aquifer::{OverworldFluidPicker, NoiseBasedAquifer};

        let mut chunk = Chunk::new(chunk_x, chunk_z);

        // Cell configuration matching Java Edition
        let cell_width = 4;  // blocks per cell in XZ
        let cell_height = 8; // blocks per cell in Y
        let min_y = -64;
        let height = 384;

        // Create FlatCacheGrid for AOT compiled Y-independent values
        // This pre-computes all FlatCache nodes (continents, erosion, temperature, etc.)
        // at the 5x5 quart grid positions for this chunk
        let grid = FlatCacheGrid::new(chunk_x, chunk_z, &self.noises);

        // Pre-compute all 256 ColumnContext values for the chunk.
        // This implements Java's Cache2D memoization pattern at chunk scale:
        // instead of computing ColumnContext per-cell (which caused ~281 calls with
        // expensive spline evaluations), we compute all 256 positions ONCE upfront.
        // This reduces spline evaluations from ~140,000 to 256 per chunk.
        let col_grid = ColumnContextGrid::new(chunk_x, chunk_z, &self.noises, &grid);

        // Create aquifer system for proper underground fluid handling
        // The aquifer determines when to place water/lava vs air in caves
        // OPTIMIZATION: Pass col_grid so aquifer can reuse pre-computed ColumnContexts
        // OPTIMIZATION: Use generic FluidPicker instead of Box to avoid heap allocation
        let fluid_picker = OverworldFluidPicker::new(Self::SEA_LEVEL);
        let mut aquifer = NoiseBasedAquifer::new(
            chunk_x,
            chunk_z,
            min_y,
            height,
            &self.noises,
            &grid,
            &col_grid,
            self.seed,
            fluid_picker,
        );

        // Create OreVeinifier for large ore vein generation (copper/iron with granite/tuff filler)
        // Uses simplified vein_toggle, vein_ridged, vein_gap density functions (no FlatCache/ColumnContext needed)
        let ore_veinifier = OreVeinifier::new(&self.noises, self.ore_random.clone());

        // Create CachingNoiseChunk for interpolation-based generation
        let mut noise_chunk = CachingNoiseChunk::new(
            chunk_x,
            chunk_z,
            cell_width,
            cell_height,
            min_y,
            height,
        );

        // Cell counts
        let cell_count_xz = noise_chunk.cell_count_xz();
        let cell_count_y = noise_chunk.cell_count_y();

        // Initialize interpolator with first X slice using AOT compiled functions
        // Uses pre-computed col_grid for O(1) column context lookups
        noise_chunk.initialize_for_first_cell_x_aot(&grid, &col_grid, &self.noises);

        // X-outer loop over cells
        for cell_x in 0..cell_count_xz {
            // Advance to next X cell using AOT compiled functions
            // Uses pre-computed col_grid for O(1) column context lookups
            noise_chunk.advance_cell_x_aot(cell_x as i32, &grid, &col_grid, &self.noises);

            // Z-middle loop
            for cell_z in 0..cell_count_xz {
                let base_block_z = chunk_z * 16 + (cell_z as i32) * cell_width;

                // Y descending for efficient surface detection
                for cell_y in (0..cell_count_y).rev() {
                    // Select this cell's corner values from the slices
                    noise_chunk.select_cell_yz(cell_y, cell_z);

                    // Iterate blocks within cell (Y descending for surface)
                    for y_in_cell in (0..cell_height as i32).rev() {
                        let block_y = min_y + (cell_y as i32) * cell_height + y_in_cell;

                        // Update Y interpolation
                        noise_chunk.update_for_y(block_y);

                        for x_in_cell in 0..cell_width {
                            let block_x = chunk_x * 16 + (cell_x as i32) * cell_width + x_in_cell;

                            // Update X interpolation and get densities
                            noise_chunk.update_for_x(block_x);
                            let densities = noise_chunk.get_densities_4z();
                            let densities_arr = densities.to_array();

                            // Check if all positive (all solid) - fast path
                            let all_positive = densities.simd_gt(f64x4::splat(0.0)).all();

                            let local_x = ((cell_x as i32) * cell_width + x_in_cell) as u8;

                            // Check if we're in ore vein Y range (-60 to 50) to avoid function call overhead
                            let in_vein_range = block_y >= -60 && block_y <= 50;

                            if all_positive {
                                // Fast path: all 4 blocks are solid
                                if in_vein_range {
                                    // Check veinifier for ore veins
                                    for z_in_cell in 0..4i32 {
                                        let local_z = ((cell_z as i32) * cell_width + z_in_cell) as u8;
                                        let block_z = base_block_z + z_in_cell;
                                        let ctx = FunctionContext::new(block_x, block_y, block_z);
                                        let block = ore_veinifier.compute(&ctx).unwrap_or(*blocks::STONE);
                                        chunk.set_block(local_x, block_y as i16, local_z, block);
                                    }
                                } else {
                                    // Outside vein range - just place stone
                                    for z_in_cell in 0..4i32 {
                                        let local_z = ((cell_z as i32) * cell_width + z_in_cell) as u8;
                                        chunk.set_block(local_x, block_y as i16, local_z, *blocks::STONE);
                                    }
                                }
                            } else {
                                // Process each block - use aquifer for non-solid, veinifier for solid
                                for z_in_cell in 0..4i32 {
                                    let density = densities_arr[z_in_cell as usize];
                                    let local_z = ((cell_z as i32) * cell_width + z_in_cell) as u8;
                                    let block_z = base_block_z + z_in_cell;

                                    if density > 0.0 {
                                        // Solid block - check veinifier for ore veins (if in range)
                                        if in_vein_range {
                                            let ctx = FunctionContext::new(block_x, block_y, block_z);
                                            let block = ore_veinifier.compute(&ctx).unwrap_or(*blocks::STONE);
                                            chunk.set_block(local_x, block_y as i16, local_z, block);
                                        } else {
                                            chunk.set_block(local_x, block_y as i16, local_z, *blocks::STONE);
                                        }
                                    } else {
                                        // Use aquifer to determine what to place (water/lava/air)
                                        // This matches Java's behavior - aquifer handles all non-solid blocks
                                        // including ocean water via globalFluidPicker
                                        let ctx = FunctionContext::new(block_x, block_y, block_z);
                                        if let Some(block_id) = aquifer.compute_substance(&ctx, density) {
                                            chunk.set_block(local_x, block_y as i16, local_z, block_id);
                                        }
                                        // None from aquifer means air - default, no need to set
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Swap slices after processing this X column
            noise_chunk.swap_slices();
        }

        // Apply surface rules
        self.surface_system.build_surface(&mut chunk, chunk_x, chunk_z);

        // Sample center biome
        let center_biome = self.get_biome(chunk_x * 16 + 8, chunk_z * 16 + 8);
        chunk.set_biome(Self::to_bedrock_biome_id(center_biome));

        chunk
    }


    /// Carve caves into the chunk using vanilla worm algorithm.
    fn carve_caves<F: super::aquifer::FluidPicker>(&self, chunk: &mut Chunk, chunk_x: i32, chunk_z: i32, aquifer: &mut NoiseBasedAquifer<F>) {
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
                            chunk, chunk_x, chunk_z, &mut rng, start_x, start_y, start_z, aquifer,
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
                            aquifer,
                        );
                    }
                }
            }
        }
    }

    /// Carve a large cave room.
    fn carve_cave_room<F: super::aquifer::FluidPicker>(
        &self,
        chunk: &mut Chunk,
        chunk_x: i32,
        chunk_z: i32,
        rng: &mut JavaRandom,
        x: f64,
        y: f64,
        z: f64,
        aquifer: &mut NoiseBasedAquifer<F>,
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
            aquifer,
        );
    }

    /// Carve a cave tunnel (worm algorithm from vanilla).
    fn carve_cave_tunnel<F: super::aquifer::FluidPicker>(
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
        aquifer: &mut NoiseBasedAquifer<F>,
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
                    aquifer,
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
                    aquifer,
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
                    // IMPORTANT: Clamp to chunk bounds [0, 16) to prevent accessing neighboring chunks
                    let min_x = ((x - radius).floor() as i32 - chunk_x * 16).clamp(0, 16);
                    let max_x = ((x + radius).floor() as i32 - chunk_x * 16 + 1).clamp(0, 16);
                    let min_y = ((y - v_radius).floor() as i32).clamp(1, 248);
                    let max_y = ((y + v_radius).floor() as i32 + 1).clamp(1, 248);
                    let min_z = ((z - radius).floor() as i32 - chunk_z * 16).clamp(0, 16);
                    let max_z = ((z + radius).floor() as i32 - chunk_z * 16 + 1).clamp(0, 16);

                    for lx in min_x..max_x {
                        // Safety check: ensure lx is within chunk bounds
                        if lx < 0 || lx >= 16 {
                            continue;
                        }
                        let dx = ((lx + chunk_x * 16) as f64 + 0.5 - x) / radius;

                        for lz in min_z..max_z {
                            // Safety check: ensure lz is within chunk bounds
                            if lz < 0 || lz >= 16 {
                                continue;
                            }
                            let dz = ((lz + chunk_z * 16) as f64 + 0.5 - z) / radius;

                            if dx * dx + dz * dz < 1.0 {
                                for ly in (min_y..max_y).rev() {
                                    let dy = ((ly - 1) as f64 + 0.5 - y) / v_radius;

                                    if dy > -0.7 && dx * dx + dy * dy + dz * dz < 1.0 {
                                        let current =
                                            chunk.get_block(lx as u8, ly as i16, lz as u8);
                                        // Only carve stone, dirt, grass - not water or bedrock
                                        if current != *blocks::WATER && current != *blocks::BEDROCK
                                        {
                                            // Java carvers use aquifer.computeSubstance(pos, 0.0)
                                            // density=0.0 forces aquifer to decide fluid vs air
                                            // Java: lava_level is "above_bottom: 8" = -64 + 8 = -56
                                            const CARVER_LAVA_LEVEL: i32 = -56;

                                            let world_x = lx + chunk_x * 16;
                                            let world_z = lz + chunk_z * 16;
                                            let ctx = FunctionContext::new(world_x, ly, world_z);

                                            // Java carvers use aquifer.computeSubstance(pos, 0.0)
                                            // density=0.0 forces aquifer to decide fluid vs air
                                            let block = if ly <= CARVER_LAVA_LEVEL {
                                                *blocks::LAVA
                                            } else {
                                                match aquifer.compute_substance(&ctx, 0.0) {
                                                    Some(block_id) => block_id,
                                                    None => continue, // Barrier - skip carving this block
                                                }
                                            };
                                            chunk.set_block(lx as u8, ly as i16, lz as u8, block);
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
    fn carve_ravines<F: super::aquifer::FluidPicker>(&self, chunk: &mut Chunk, chunk_x: i32, chunk_z: i32, aquifer: &mut NoiseBasedAquifer<F>) {
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
                    aquifer,
                );
            }
        }
    }

    /// Carve a ravine tunnel (similar to cave but taller and shallower)
    fn carve_ravine_tunnel<F: super::aquifer::FluidPicker>(
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
        aquifer: &mut NoiseBasedAquifer<F>,
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
                    // IMPORTANT: Clamp to chunk bounds [0, 16) to prevent accessing neighboring chunks
                    let min_x = ((x - h_radius).floor() as i32 - chunk_x * 16).clamp(0, 16);
                    let max_x = ((x + h_radius).floor() as i32 - chunk_x * 16 + 1).clamp(0, 16);
                    let min_y = ((y - v_radius).floor() as i32).clamp(1, 248);
                    let max_y = ((y + v_radius).floor() as i32 + 1).clamp(1, 248);
                    let min_z = ((z - h_radius).floor() as i32 - chunk_z * 16).clamp(0, 16);
                    let max_z = ((z + h_radius).floor() as i32 - chunk_z * 16 + 1).clamp(0, 16);

                    for lx in min_x..max_x {
                        // Safety check: ensure lx is within chunk bounds
                        if lx < 0 || lx >= 16 {
                            continue;
                        }
                        let dx = ((lx + chunk_x * 16) as f64 + 0.5 - x) / h_radius;

                        for lz in min_z..max_z {
                            // Safety check: ensure lz is within chunk bounds
                            if lz < 0 || lz >= 16 {
                                continue;
                            }
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
                                            // Java carvers use aquifer.computeSubstance(pos, 0.0)
                                            // density=0.0 forces aquifer to decide fluid vs air
                                            // Java: lava_level is "above_bottom: 8" = -64 + 8 = -56
                                            const CARVER_LAVA_LEVEL: i32 = -56;

                                            let world_x = lx + chunk_x * 16;
                                            let world_z = lz + chunk_z * 16;
                                            let ctx = FunctionContext::new(world_x, ly, world_z);

                                            // Java carvers use aquifer.computeSubstance(pos, 0.0)
                                            // density=0.0 forces aquifer to decide fluid vs air
                                            let block = if ly <= CARVER_LAVA_LEVEL {
                                                *blocks::LAVA
                                            } else {
                                                match aquifer.compute_substance(&ctx, 0.0) {
                                                    Some(block_id) => block_id,
                                                    None => continue, // Barrier - skip carving this block
                                                }
                                            };
                                            chunk.set_block(lx as u8, ly as i16, lz as u8, block);
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
}
