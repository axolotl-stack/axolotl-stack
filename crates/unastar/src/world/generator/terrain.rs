//! Java-style generator: 3D terrain from the AOT-compiled noise router, aquifers, ore
//! veins and surface rules.
//!
//! This module implements Java Edition-style terrain generation with:
//! - Density functions for 3D terrain shaping (overhangs, caves, etc.)
//! - Cell-based interpolation for performance
//! - Aquifer system for underground water/lava pockets
//! - Surface rules for biome-based block placement

use super::biome_source;
use super::density::{
    CachingNoiseChunk, ColumnContext, ColumnContextGrid, FlatCacheGrid, FunctionContext,
    NoiseRegistry, compute_final_density,
};
use super::ore_veinifier::OreVeinifier;
use super::pipeline::{Generator, SpawnPoint};
use super::proto::{NativeBiome, ProtoChunk};
use super::region::DecorationRegion;
use super::surface::SurfaceSystem;
use super::xoroshiro::PositionalRandomFactory;
use crate::world::ChunkPos;
use crate::world::chunk::{Chunk, blocks};
use std::simd::prelude::*;
use unastar_noise::java_biomes::JavaBiome;
use unastar_noise::overworld_settings;

/// Java-style terrain generator using 3D density functions.
///
/// This generator produces terrain matching Java Edition's modern world generation:
/// - 3D density-based terrain with overhangs and natural caves
/// - Aquifer system for underground water and lava pockets
/// - Surface rules for biome-specific block placement
pub struct JavaGenerator {
    /// World seed
    pub seed: i64,
    /// Noise registry with instantiated noises from seed
    noises: NoiseRegistry,
    /// Surface rules system for biome-based surface blocks
    surface_system: SurfaceSystem,
    /// Ore vein positional random (`fromHashOf("minecraft:ore").forkPositional()`).
    ore_random: PositionalRandomFactory,
}

impl JavaGenerator {
    /// Sea level
    pub const SEA_LEVEL: i32 = overworld_settings::SEA_LEVEL;

    /// Create a new Java-style generator with the given seed.
    pub fn new(seed: i64) -> Self {
        // Create noise registry with all noises instantiated from seed
        let noises = NoiseRegistry::new(seed);

        // Compiled surface rule pass (seed-derived randoms and clay bands).
        let surface_system = SurfaceSystem::new(seed);

        // `fromHashOf("minecraft:ore").forkPositional()` of the world's base factory.
        let ore_random =
            PositionalRandomFactory::new(seed).fork_from_hash(unastar_noise::random_hashes::ORE);

        Self {
            seed,
            noises,
            surface_system,
            ore_random,
        }
    }

    /// Find a safe spawn location by sampling terrain.
    ///
    /// Searches outward from origin for a location above sea level.
    pub fn find_safe_spawn(&self) -> (i32, i32, i32) {
        // Cache grids per chunk to avoid recreating them
        let mut grid_cache: std::collections::HashMap<(i32, i32), FlatCacheGrid> =
            std::collections::HashMap::new();

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
                    let grid = grid_cache
                        .entry((chunk_x, chunk_z))
                        .or_insert_with(|| FlatCacheGrid::new(chunk_x, chunk_z, &self.noises));

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

    /// Terrain blocks of a chunk using 3D density functions with cell-based caching.
    ///
    /// `grid` and `col_grid` are the chunk's flat (quart) and column caches, shared with
    /// the biome source. The generation process:
    /// 1. Create NoiseBasedAquifer for underground fluid handling
    /// 2. Create a CachingNoiseChunk for cell-based caching and interpolation
    /// 3. Initialize interpolators with first X slice using AOT compiled functions
    /// 4. Traverse cells (4x8x4 blocks) in X-outer, Z-middle, Y-inner order
    /// 5. Use trilinear interpolation for density (compute at 8 corners, interpolate interior)
    /// 6. Use aquifer system to determine fluid placement when density <= 0
    /// 7. Apply surface rules for biome-specific blocks
    pub fn generate_terrain(
        &self,
        chunk_x: i32,
        chunk_z: i32,
        grid: &FlatCacheGrid,
        col_grid: &ColumnContextGrid,
    ) -> Chunk {
        use super::aquifer::{NoiseBasedAquifer, OverworldFluidPicker};

        let mut chunk = Chunk::new(chunk_x, chunk_z);

        let cell_width = overworld_settings::CELL_WIDTH;
        let cell_height = overworld_settings::CELL_HEIGHT;
        let min_y = overworld_settings::MIN_Y;
        let height = overworld_settings::HEIGHT;

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
            grid,
            col_grid,
            self.seed,
            fluid_picker,
        );

        // Create OreVeinifier for large ore vein generation (copper/iron with granite/tuff filler)
        let ore_veinifier = OreVeinifier::new(&self.noises, self.ore_random.clone());

        // Create CachingNoiseChunk for interpolation-based generation
        let mut noise_chunk =
            CachingNoiseChunk::new(chunk_x, chunk_z, cell_width, cell_height, min_y, height);

        // Cell counts
        let cell_count_xz = noise_chunk.cell_count_xz();
        let cell_count_y = noise_chunk.cell_count_y();

        // Initialize interpolator with first X slice using AOT compiled functions
        // Uses pre-computed col_grid for O(1) column context lookups
        noise_chunk.initialize_for_first_cell_x_aot(grid, col_grid, &self.noises);

        // X-outer loop over cells
        for cell_x in 0..cell_count_xz {
            // Advance to next X cell using AOT compiled functions
            // Uses pre-computed col_grid for O(1) column context lookups
            noise_chunk.advance_cell_x_aot(cell_x as i32, grid, col_grid, &self.noises);

            // Z-middle loop
            for cell_z in 0..cell_count_xz {
                let base_block_z = chunk_z * 16 + (cell_z as i32) * cell_width;

                // Y descending for efficient surface detection
                for cell_y in (0..cell_count_y).rev() {
                    // Select this cell's corner values from the slices
                    noise_chunk.select_cell_yz(cell_y, cell_z);

                    // Iterate blocks within cell (Y descending for surface)
                    for y_in_cell in (0..cell_height).rev() {
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
                            let in_vein_range = (-60..=50).contains(&block_y);

                            if all_positive {
                                // Fast path: all 4 blocks are solid
                                if in_vein_range {
                                    // Check veinifier for ore veins
                                    for z_in_cell in 0..4i32 {
                                        let local_z =
                                            ((cell_z as i32) * cell_width + z_in_cell) as u8;
                                        let block_z = base_block_z + z_in_cell;
                                        let ctx = FunctionContext::new(block_x, block_y, block_z);
                                        let block =
                                            ore_veinifier.compute(&ctx).unwrap_or(*blocks::STONE);
                                        chunk.set_block(local_x, block_y as i16, local_z, block);
                                    }
                                } else {
                                    // Outside vein range - just place stone
                                    for z_in_cell in 0..4i32 {
                                        let local_z =
                                            ((cell_z as i32) * cell_width + z_in_cell) as u8;
                                        chunk.set_block(
                                            local_x,
                                            block_y as i16,
                                            local_z,
                                            *blocks::STONE,
                                        );
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
                                            let ctx =
                                                FunctionContext::new(block_x, block_y, block_z);
                                            let block = ore_veinifier
                                                .compute(&ctx)
                                                .unwrap_or(*blocks::STONE);
                                            chunk.set_block(
                                                local_x,
                                                block_y as i16,
                                                local_z,
                                                block,
                                            );
                                        } else {
                                            chunk.set_block(
                                                local_x,
                                                block_y as i16,
                                                local_z,
                                                *blocks::STONE,
                                            );
                                        }
                                    } else {
                                        // The aquifer decides air, water or lava; `None` is a
                                        // barrier between aquifers, which falls through to the
                                        // ore veins and then the default block, like solid
                                        // density does.
                                        let ctx = FunctionContext::new(block_x, block_y, block_z);
                                        let block = match aquifer.compute_substance(&ctx, density) {
                                            Some(block) => block,
                                            None if in_vein_range => ore_veinifier
                                                .compute(&ctx)
                                                .unwrap_or(*blocks::STONE),
                                            None => *blocks::STONE,
                                        };
                                        if block != *blocks::AIR {
                                            chunk.set_block(
                                                local_x,
                                                block_y as i16,
                                                local_z,
                                                block,
                                            );
                                        }
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

        chunk
    }
}

impl Generator for JavaGenerator {
    fn seed(&self) -> i64 {
        self.seed
    }

    fn generate_proto(&self, pos: ChunkPos) -> ProtoChunk {
        // Flat (quart) and column caches shared by terrain and biomes.
        let grid = FlatCacheGrid::new(pos.x, pos.z, &self.noises);
        let col_grid = ColumnContextGrid::new(pos.x, pos.z, &self.noises, &grid);
        let biomes = biome_source::fill_chunk(&self.noises, pos, &grid, &col_grid);
        let mut chunk = self.generate_terrain(pos.x, pos.z, &grid, &col_grid);
        self.surface_system
            .build_surface(&mut chunk, pos, &self.noises, &grid, &biomes);
        ProtoChunk { pos, chunk, biomes }
    }

    fn decorates(&self) -> bool {
        false
    }

    fn decorate(&self, _region: &mut DecorationRegion<'_>) {}

    fn block_classes(&self) -> &'static [u8] {
        &unastar_noise::java_blocks::BEDROCK_STATE_CLASSES
    }

    fn bedrock_biome(&self, biome: NativeBiome) -> u32 {
        JavaBiome::from_index(biome)
            .expect("Java-style protos store JavaBiome discriminants")
            .data()
            .bedrock_id
    }

    fn find_spawn(&self) -> SpawnPoint {
        let (x, y, z) = self.find_safe_spawn();
        SpawnPoint { x, y, z }
    }
}
