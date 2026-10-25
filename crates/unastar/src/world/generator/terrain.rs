//! Java-style generator: 3D terrain from the AOT-compiled noise router, aquifers, ore
//! veins, 3D biomes, surface rules and carvers.
//!
//! This module implements Java Edition-style terrain generation with:
//! - Density functions for 3D terrain shaping (overhangs, caves, etc.)
//! - Cell-based interpolation for performance
//! - Aquifer system for underground water/lava pockets
//! - Surface rules for biome-based block placement
//! - Cave and canyon carvers

use super::aquifer::{NoiseBasedAquifer, OverworldFluidPicker};
use super::biome_source;
use super::carver::{self, CarveContext};
use super::density::{
    CachingNoiseChunk, ColumnContextGrid, FlatCacheGrid, FunctionContext, NoiseRegistry,
};
use super::ore_veinifier::OreVeinifier;
use super::pipeline::{Generator, SpawnPoint};
use super::proto::{DenseBlocks, NativeBiome, ProtoChunk, QuartBiomes};
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

    /// Shared noise-stage proto: density, aquifers, ore veins and quart biomes,
    /// before edition-specific surface builders and explicit carvers.
    ///
    /// Uses this generator's seeded noise registry; consumers must not create
    /// another registry to access the shared terrain core.
    pub fn generate_noise_proto(&self, pos: ChunkPos) -> ProtoChunk {
        let (dense, biomes) = self.generate_noise_blocks(pos);
        let chunk = dense.to_chunk(pos, *blocks::AIR);
        ProtoChunk { pos, chunk, biomes }
    }

    /// Dense form of the shared noise stage, so another edition can apply
    /// its surface/carvers before packing block palettes.
    pub fn generate_noise_blocks(&self, pos: ChunkPos) -> (DenseBlocks, QuartBiomes) {
        let grid = FlatCacheGrid::new(pos.x, pos.z, &self.noises);
        let col_grid = ColumnContextGrid::new(pos.x, pos.z, &self.noises, &grid);
        let biomes = biome_source::fill_chunk(&self.noises, pos, &grid, &col_grid);
        let mut aquifer = self.aquifer(pos, &grid, &col_grid);
        let dense = self.generate_terrain(pos.x, pos.z, &grid, &col_grid, &mut aquifer);
        (dense, biomes)
    }

    /// Native Java biome at the quart containing an absolute block position.
    pub fn sample_biome(&self, x: i32, y: i32, z: i32) -> NativeBiome {
        biome_source::sample_biome(&self.noises, x >> 2, y >> 2, z >> 2) as NativeBiome
    }

    /// First block above motion-blocking noise terrain, in `z * 16 + x`
    /// order. Surface rules and explicit carvers have not run.
    ///
    /// Structure callers should cache this chunk-level oracle for their
    /// placement run rather than generating the same terrain for each column.
    pub fn noise_heightmap(&self, pos: ChunkPos) -> [i32; 256] {
        let (dense, _) = self.generate_noise_blocks(pos);
        let classes = &unastar_noise::java_blocks::BEDROCK_STATE_CLASSES;
        std::array::from_fn(|i| {
            let (x, z) = ((i % 16) as u8, (i / 16) as u8);
            (overworld_settings::MIN_Y..overworld_settings::MIN_Y + overworld_settings::HEIGHT)
                .rev()
                .find(|&y| {
                    classes[dense.get_block(x, y as i16, z) as usize]
                        & unastar_noise::block_class::BLOCKS_MOTION
                        != 0
                })
                .map_or(overworld_settings::MIN_Y, |y| y + 1)
        })
    }

    /// The chunk's aquifer (shared by the noise fill and the carvers).
    pub fn aquifer<'a>(
        &'a self,
        pos: ChunkPos,
        grid: &'a FlatCacheGrid,
        col_grid: &'a ColumnContextGrid,
    ) -> NoiseBasedAquifer<'a, OverworldFluidPicker> {
        NoiseBasedAquifer::new(
            pos.x,
            pos.z,
            overworld_settings::MIN_Y,
            overworld_settings::HEIGHT,
            &self.noises,
            grid,
            col_grid,
            self.seed,
            OverworldFluidPicker::new(Self::SEA_LEVEL),
        )
    }

    /// Terrain blocks of a chunk using 3D density functions with cell-based caching.
    ///
    /// `grid` and `col_grid` are the chunk's flat (quart) and column caches, shared with
    /// the biome source. The generation process:
    /// 1. Create a CachingNoiseChunk for cell-based caching and interpolation
    /// 2. Initialize interpolators with first X slice using AOT compiled functions
    /// 3. Traverse cells (4x8x4 blocks) in X-outer, Z-middle, Y-inner order
    /// 4. Use trilinear interpolation for density (compute at 8 corners, interpolate interior)
    /// 5. Use the aquifer to place air, water or lava where density <= 0
    pub fn generate_terrain(
        &self,
        chunk_x: i32,
        chunk_z: i32,
        grid: &FlatCacheGrid,
        col_grid: &ColumnContextGrid,
        aquifer: &mut NoiseBasedAquifer<'_, OverworldFluidPicker>,
    ) -> DenseBlocks {
        // Air until filled; the paletted chunk is packed once all stages ran.
        let mut chunk = DenseBlocks::filled(*blocks::AIR);

        let cell_width = overworld_settings::CELL_WIDTH;
        let cell_height = overworld_settings::CELL_HEIGHT;
        let min_y = overworld_settings::MIN_Y;
        let height = overworld_settings::HEIGHT;

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

/// Carving state of one proto chunk: its aquifer and what the surface rule needs for
/// exposed dirt.
struct JavaCarveContext<'a, 'g> {
    generator: &'a JavaGenerator,
    aquifer: &'a mut NoiseBasedAquifer<'g, OverworldFluidPicker>,
    pos: ChunkPos,
    grid: &'a FlatCacheGrid,
    biomes: &'a QuartBiomes,
}

impl CarveContext for JavaCarveContext<'_, '_> {
    fn carve_substance(&mut self, x: i32, y: i32, z: i32) -> Option<u32> {
        self.aquifer.carve_substance(x, y, z)
    }

    fn top_material(
        &mut self,
        blocks: &DenseBlocks,
        x: i32,
        y: i32,
        z: i32,
        has_fluid: bool,
    ) -> Option<u32> {
        self.generator.surface_system.top_material(
            blocks,
            self.pos,
            &self.generator.noises,
            self.grid,
            self.biomes,
            x,
            y,
            z,
            has_fluid,
        )
    }
}

impl Generator for JavaGenerator {
    fn seed(&self) -> i64 {
        self.seed
    }

    fn generate_proto(&self, pos: ChunkPos) -> ProtoChunk {
        // Flat (quart) and column caches shared by terrain, biomes, surface and carvers.
        let grid = FlatCacheGrid::new(pos.x, pos.z, &self.noises);
        let col_grid = ColumnContextGrid::new(pos.x, pos.z, &self.noises, &grid);
        let biomes = biome_source::fill_chunk(&self.noises, pos, &grid, &col_grid);
        let mut aquifer = self.aquifer(pos, &grid, &col_grid);
        let mut dense = self.generate_terrain(pos.x, pos.z, &grid, &col_grid, &mut aquifer);
        self.surface_system
            .build_surface(&mut dense, pos, &self.noises, &grid, &biomes);
        let mut carving = JavaCarveContext {
            generator: self,
            aquifer: &mut aquifer,
            pos,
            grid: &grid,
            biomes: &biomes,
        };
        carver::carve_chunk(self.seed, &mut dense, pos, &mut carving);
        let chunk = dense.to_chunk(pos, *blocks::AIR);
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

    fn find_spawn(&self, chunk_at: &dyn Fn(ChunkPos) -> Chunk) -> SpawnPoint {
        let start = biome_source::spawn_search_start(&self.noises);
        super::spawn::java_spawn(start, chunk_at, self.block_classes())
    }
}

#[cfg(test)]
mod shared_noise_tests {
    use super::*;

    /// A carver's aquifer must not depend on having populated the noise-stage
    /// cache first. Other edition surfaces run between these stages.
    #[test]
    fn separated_noise_stage_preserves_java_terrain_and_quart_boundaries() {
        std::thread::Builder::new()
            .stack_size(64 << 20)
            .spawn(check_separated_stages)
            .unwrap()
            .join()
            .unwrap();
    }

    fn check_separated_stages() {
        let generator = JavaGenerator::new(12345);
        for pos in [
            ChunkPos::new(0, 0),
            ChunkPos::new(-2, 1),
            ChunkPos::new(8, -3),
        ] {
            let (mut dense, biomes) = generator.generate_noise_blocks(pos);
            let heights = generator.noise_heightmap(pos);
            let classes = &unastar_noise::java_blocks::BEDROCK_STATE_CLASSES;
            for (i, &top) in heights.iter().enumerate() {
                let (x, z) = ((i % 16) as u8, (i / 16) as u8);
                if top > overworld_settings::MIN_Y {
                    assert_ne!(
                        classes[dense.get_block(x, (top - 1) as i16, z) as usize]
                            & unastar_noise::block_class::BLOCKS_MOTION,
                        0
                    );
                }
                for y in top..overworld_settings::MIN_Y + overworld_settings::HEIGHT {
                    assert_eq!(
                        classes[dense.get_block(x, y as i16, z) as usize]
                            & unastar_noise::block_class::BLOCKS_MOTION,
                        0
                    );
                }
            }
            for (x, y, z) in [(0, -64, 0), (15, 319, 15), (4, 64, 8)] {
                assert_eq!(
                    biomes.at_block(x, y, z),
                    generator.sample_biome(pos.x * 16 + i32::from(x), y, pos.z * 16 + i32::from(z)),
                );
            }
            let grid = FlatCacheGrid::new(pos.x, pos.z, &generator.noises);
            let col_grid = ColumnContextGrid::new(pos.x, pos.z, &generator.noises, &grid);
            let mut aquifer = generator.aquifer(pos, &grid, &col_grid);
            generator.surface_system.build_surface(
                &mut dense,
                pos,
                &generator.noises,
                &grid,
                &biomes,
            );
            let mut carving = JavaCarveContext {
                generator: &generator,
                aquifer: &mut aquifer,
                pos,
                grid: &grid,
                biomes: &biomes,
            };
            carver::carve_chunk(generator.seed, &mut dense, pos, &mut carving);
            let full = generator.generate_proto(pos);
            assert_eq!(full.biomes, biomes);
            for y in
                overworld_settings::MIN_Y..overworld_settings::MIN_Y + overworld_settings::HEIGHT
            {
                for z in 0..16 {
                    for x in 0..16 {
                        assert_eq!(
                            dense.get_block(x, y as i16, z),
                            full.chunk.get_block(x, y as i16, z),
                            "stage split changed ({x},{y},{z}) in {pos:?}"
                        );
                    }
                }
            }
        }
    }
}
