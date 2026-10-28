//! Java-style generator: 3D terrain from the AOT-compiled noise router, aquifers, ore
//! veins, 3D biomes, surface rules and carvers.
//!
//! This module implements Java Edition-style terrain generation with:
//! - Density functions for 3D terrain shaping (overhangs, caves, etc.)
//! - Cell-based interpolation for performance
//! - Aquifer system for underground water/lava pockets
//! - Surface rules for biome-based block placement
//! - Cave and canyon carvers

mod native;
mod columns;
mod source;

use std::collections::HashSet;
use std::sync::Mutex;

use super::JavaDimension;
#[cfg(feature = "bedrock-worldgen")]
use super::aquifer::{NoiseBasedAquifer, OverworldFluidPicker};
use super::biome_source;
use super::density::{NativeNoiseRegistry, NoiseRegistry};
#[cfg(feature = "bedrock-worldgen")]
use super::density::{
    CachingNoiseChunk, ColumnContextGrid, FlatCacheGrid, FunctionContext,
};
#[cfg(feature = "bedrock-worldgen")]
use super::ore_veinifier::OreVeinifier;
use super::pipeline::{Generator, SpawnPoint};
use super::proto::{NativeBiome, ProtoChunk};
use super::region::DecorationRegion;
use super::surface::SurfaceSystem;
use super::xoroshiro::PositionalRandomFactory;
use crate::world::ChunkPos;
use crate::world::chunk::{Chunk, blocks};
#[cfg(feature = "bedrock-worldgen")]
use crate::world::chunk::BlockStateSpace;
#[cfg(feature = "bedrock-worldgen")]
use crate::world::generator::proto::DenseBlocks;
#[cfg(feature = "bedrock-worldgen")]
use std::simd::prelude::*;
use unastar_noise::java_biomes::JavaBiome;
use unastar_noise::overworld_settings;
#[cfg(feature = "bedrock-worldgen")]
use unastar_noise::profile::{self, Stage};
use unastar_noise::features::java_blocks::states;

/// The shared aquifer's output domain is exactly ordinary air and source water/lava.
/// Translate that closed producer contract, never an arbitrary Bedrock state or a
/// many-to-one block map. No aquifer RNG/cache behavior is changed here.
fn java_aquifer_substance(state: u32) -> u32 {
    if state == *blocks::AIR {
        u32::from(states::AIR.0)
    } else if state == *blocks::WATER {
        u32::from(states::WATER.0)
    } else if state == *blocks::LAVA {
        u32::from(states::LAVA.0)
    } else {
        panic!("aquifer emitted a state outside its air/source-fluid domain: {state}");
    }
}

/// Native block-affecting terrain stages, in Java chunk-status order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum JavaTerrainStage {
    Empty,
    StructureStarts,
    StructureReferences,
    Biomes,
    Noise,
    Surface,
    Carvers,
}

/// Native world inputs that control structure and biome-feature placement.
/// Disabling biome features models an explicitly verified empty-feature datapack,
/// not ordinary vanilla generation.
#[derive(Debug, Clone, Copy)]
pub struct JavaGenerationOptions {
    pub dimension: JavaDimension,
    pub generate_structures: bool,
    pub biome_features: bool,
}

impl Default for JavaGenerationOptions {
    fn default() -> Self {
        Self {
            dimension: JavaDimension::Overworld,
            generate_structures: true,
            biome_features: true,
        }
    }
}

enum TerrainBackend {
    NativeJava(NativeNoiseRegistry),
    #[cfg(feature = "bedrock-worldgen")]
    SharedBedrock(NoiseRegistry),
}

/// Java-style terrain generator using 3D density functions.
///
/// This generator produces terrain matching Java Edition's modern world generation:
/// - 3D density-based terrain with overhangs and natural caves
/// - Aquifer system for underground water and lava pockets
/// - Surface rules for biome-specific block placement
pub struct JavaGenerator {
    /// World seed
    pub seed: i64,
    /// Surface rules system for biome-based surface blocks
    surface_system: SurfaceSystem,
    /// Ore vein positional random (`fromHashOf("minecraft:ore").forkPositional()`).
    ore_random: PositionalRandomFactory,
    /// Structure starts and noise-only column heights (Java-style structures).
    structures: super::structures::JavaStructures,
    options: JavaGenerationOptions,
    backend: TerrainBackend,
    /// Native source chunks cache their first carver-biome lookup. AOT verifies
    /// equal carver lists across each preset, so only cache presence is needed.
    carver_biomes: Mutex<HashSet<ChunkPos>>,
}

impl JavaGenerator {
    /// Sea level
    pub const SEA_LEVEL: i32 = overworld_settings::SEA_LEVEL;

    /// Create a new Java-style generator with the given seed.
    pub fn new(seed: i64) -> Self {
        Self::with_options(seed, JavaGenerationOptions::default())
    }

    /// Construct with explicit native placement inputs, before any chunk cache
    /// or per-thread biome query history is populated.
    pub fn with_options(seed: i64, options: JavaGenerationOptions) -> Self {
        macro_rules! native_registry {
            ($settings:path) => {{
                use $settings as settings;
                let params = settings::BLENDED_NOISE;
                NoiseRegistry::with_router_settings(seed, settings::LEGACY_RANDOM_SOURCE, [
                    params.xz_scale, params.y_scale, params.xz_factor,
                    params.y_factor, params.smear_scale_multiplier,
                ])
            }};
        }
        let noises = match options.dimension {
            JavaDimension::Overworld => native_registry!(unastar_noise::overworld_settings),
            JavaDimension::Nether => native_registry!(unastar_noise::nether_settings),
            JavaDimension::End => native_registry!(unastar_noise::end_settings),
        };
        Self::with_backend(seed, options, TerrainBackend::NativeJava(noises))
    }

    /// Preserve the calibrated shared density arithmetic for the Bedrock owner.
    /// Native Java arithmetic is selected separately, outside the voxel loop.
    #[cfg(feature = "bedrock-worldgen")]
    pub fn for_bedrock_shared_terrain(seed: i64) -> Self {
        Self::with_backend(
            seed, JavaGenerationOptions::default(), TerrainBackend::SharedBedrock(NoiseRegistry::new(seed)),
        )
    }

    fn with_backend(
        seed: i64,
        options: JavaGenerationOptions,
        backend: TerrainBackend,
    ) -> Self {
        let surface_system = SurfaceSystem::new(seed, options.dimension);

        // `fromHashOf("minecraft:ore").forkPositional()` of the world's base factory.
        let ore_random =
            PositionalRandomFactory::new(seed).fork_from_hash(unastar_noise::random_hashes::ORE);

        Self {
            seed,
            surface_system,
            ore_random,
            structures: super::structures::JavaStructures::new(options.dimension),
            options,
            backend,
            carver_biomes: Mutex::new(HashSet::new()),
        }
    }

    fn native_noises(&self) -> &NativeNoiseRegistry {
        match &self.backend {
            TerrainBackend::NativeJava(noises) => noises,
            #[cfg(feature = "bedrock-worldgen")]
            TerrainBackend::SharedBedrock(_) => panic!("native sampling requires the Java backend"),
        }
    }

    #[cfg(feature = "bedrock-worldgen")]
    fn shared_noises(&self) -> &NoiseRegistry {
        match &self.backend {
            TerrainBackend::SharedBedrock(noises) => noises,
            TerrainBackend::NativeJava(_) => panic!("calibrated sampling requires the Bedrock backend"),
        }
    }

    /// Shared noise-stage proto: density, aquifers, ore veins and quart biomes,
    /// before edition-specific surface builders and explicit carvers.
    ///
    /// Uses this generator's seeded noise registry; consumers must not create
    /// another registry to access the shared terrain core.
    pub fn generate_noise_proto(&self, pos: ChunkPos) -> ProtoChunk {
        self.generate_stage(pos, JavaTerrainStage::Noise)
    }

    /// Snapshot the exact requested native stage without running later stages.
    pub fn generate_stage(&self, pos: ChunkPos, stage: JavaTerrainStage) -> ProtoChunk {
        match &self.backend {
            TerrainBackend::NativeJava(_) if stage < JavaTerrainStage::Biomes => {
                if stage >= JavaTerrainStage::StructureStarts {
                    self.structure_starts(pos.x, pos.z);
                }
                if stage == JavaTerrainStage::StructureReferences && self.options.generate_structures {
                    let context = self.structures.context(self.seed, self);
                    self.structures.manager.references(&context, pos.x, pos.z);
                }
                // Empty section palettes are plains in every native dimension.
                // Noise-biome queries before BIOMES would mutate search history.
                self.biomes_stage(
                    pos,
                    super::proto::QuartBiomes::filled(JavaBiome::Plains as NativeBiome),
                )
            }
            TerrainBackend::NativeJava(_) => match self.options.dimension {
                JavaDimension::Overworld => self.generate_overworld_native(pos, stage),
                JavaDimension::Nether => self.generate_nether_native(pos, stage),
                JavaDimension::End => self.generate_end_native(pos, stage),
            },
            #[cfg(feature = "bedrock-worldgen")]
            TerrainBackend::SharedBedrock(_) => self.generate_shared_stage(pos, stage),
        }
    }

    #[cfg(feature = "bedrock-worldgen")]
    fn generate_shared_stage(&self, pos: ChunkPos, stage: JavaTerrainStage) -> ProtoChunk {
        assert!((JavaTerrainStage::Biomes..=JavaTerrainStage::Noise).contains(&stage),
            "the calibrated shared backend only exposes biome and noise stages");
        let noises = self.shared_noises();
        let grid = FlatCacheGrid::new(pos.x, pos.z, noises);
        let col_grid = ColumnContextGrid::new(pos.x, pos.z, noises, &grid);
        let biomes = biome_source::fill_chunk(noises, pos, &grid, &col_grid);
        if stage == JavaTerrainStage::Biomes {
            return ProtoChunk {
                pos,
                chunk: Chunk::new_with_bounds(
                    pos.x, pos.z, BlockStateSpace::Java,
                    self.options.dimension.block_bounds(),
                ),
                biomes,
            };
        }
        let mut aquifer = self.aquifer(pos, &grid, &col_grid);
        let dense = self.generate_shared_terrain(pos.x, pos.z, &grid, &col_grid, &mut aquifer);
        ProtoChunk { pos, chunk: dense.to_chunk(pos), biomes }
    }


    /// Share the density stage and climate caches without imposing Java's
    /// stateful biome-search policy on another edition or on base-height queries.
    #[cfg(feature = "bedrock-worldgen")]
    pub(crate) fn generate_noise_with<T>(
        &self,
        pos: ChunkPos,
        fill: impl FnOnce(&NoiseRegistry, ChunkPos, &FlatCacheGrid, &ColumnContextGrid) -> T,
    ) -> (DenseBlocks, T) {
        let _noise = profile::scope(Stage::Noise);
        let noises = self.shared_noises();
        let grid = FlatCacheGrid::new(pos.x, pos.z, noises);
        let col_grid = ColumnContextGrid::new(pos.x, pos.z, noises, &grid);
        let biomes = fill(noises, pos, &grid, &col_grid);
        let mut aquifer = self.aquifer(pos, &grid, &col_grid);
        let dense = self.generate_shared_terrain(pos.x, pos.z, &grid, &col_grid, &mut aquifer);
        (dense, biomes)
    }

    /// Native Java biome at the quart containing an absolute block position.
    pub fn sample_biome(&self, x: i32, y: i32, z: i32) -> NativeBiome {
        let biome = match &self.backend {
            TerrainBackend::NativeJava(noises) => biome_source::sample_biome_native(
                self.options.dimension, &noises.java_source(), x >> 2, y >> 2, z >> 2,
            ),
            #[cfg(feature = "bedrock-worldgen")]
            TerrainBackend::SharedBedrock(noises) => {
                biome_source::sample_biome(noises, x >> 2, y >> 2, z >> 2)
            }
        };
        biome as NativeBiome
    }

    /// Climate target for a foreign edition's independently owned biome lookup.
    #[cfg(feature = "bedrock-worldgen")]
    pub(crate) fn sample_climate(&self, x: i32, y: i32, z: i32) -> [i64; 7] {
        biome_source::sample_target(self.shared_noises(), x >> 2, y >> 2, z >> 2)
    }

    /// Calibrated shared-backend motion-blocking heights, cached by the Bedrock
    /// owner. Native structure queries use the dedicated column-mode sampler.
    #[cfg(feature = "bedrock-worldgen")]
    pub(crate) fn noise_heightmap(&self, pos: ChunkPos) -> [i32; 256] {
        let (dense, ()) = self.generate_noise_with(pos, |_, _, _, _| ());
        let classes = &unastar_noise::java_blocks::JAVA_STATE_CLASSES;
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

    /// The calibrated shared-backend aquifer.
    #[cfg(feature = "bedrock-worldgen")]
    fn aquifer<'a>(
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
            self.shared_noises(),
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
    #[cfg(feature = "bedrock-worldgen")]
    fn generate_shared_terrain(
        &self,
        chunk_x: i32,
        chunk_z: i32,
        grid: &FlatCacheGrid,
        col_grid: &ColumnContextGrid,
        aquifer: &mut NoiseBasedAquifer<'_, OverworldFluidPicker>,
    ) -> DenseBlocks {
        let noises = self.shared_noises();
        // Air until filled; the paletted chunk is packed once all stages ran.
        let mut chunk = DenseBlocks::filled_with_bounds(
            u32::from(states::AIR.0), BlockStateSpace::Java,
            self.options.dimension.block_bounds(),
        );

        let cell_width = overworld_settings::CELL_WIDTH;
        let cell_height = overworld_settings::CELL_HEIGHT;
        let min_y = overworld_settings::MIN_Y;
        let height = overworld_settings::HEIGHT;

        // Create OreVeinifier for large ore vein generation (copper/iron with granite/tuff filler)
        let ore_veinifier = OreVeinifier::new(noises, self.ore_random.clone());

        // Create CachingNoiseChunk for interpolation-based generation
        let mut noise_chunk =
            CachingNoiseChunk::new(chunk_x, chunk_z, cell_width, cell_height, min_y, height);

        // Cell counts
        let cell_count_xz = noise_chunk.cell_count_xz();
        let cell_count_y = noise_chunk.cell_count_y();

        // Initialize interpolator with first X slice using AOT compiled functions
        // Uses pre-computed col_grid for O(1) column context lookups
        noise_chunk.initialize_for_first_cell_x_aot(grid, col_grid, noises);

        // X-outer loop over cells
        for cell_x in 0..cell_count_xz {
            // Advance to next X cell using AOT compiled functions
            // Uses pre-computed col_grid for O(1) column context lookups
            noise_chunk.advance_cell_x_aot(cell_x as i32, grid, col_grid, noises);

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
                                    let _fluids = profile::scope(Stage::AquiferVeins);
                                    for z_in_cell in 0..4i32 {
                                        let local_z =
                                            ((cell_z as i32) * cell_width + z_in_cell) as u8;
                                        let block_z = base_block_z + z_in_cell;
                                        let ctx = FunctionContext::new(block_x, block_y, block_z);
                                        let block =
                                            ore_veinifier.compute(&ctx).unwrap_or(u32::from(states::STONE.0));
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
                                            u32::from(states::STONE.0),
                                        );
                                    }
                                }
                            } else {
                                // Process each block - use aquifer for non-solid, veinifier for solid
                                let _fluids = profile::scope(Stage::AquiferVeins);
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
                                                .unwrap_or(u32::from(states::STONE.0));
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
                                                u32::from(states::STONE.0),
                                            );
                                        }
                                    } else {
                                        // The aquifer decides air, water or lava; `None` is a
                                        // barrier between aquifers, which falls through to the
                                        // ore veins and then the default block, like solid
                                        // density does.
                                        let ctx = FunctionContext::new(block_x, block_y, block_z);
                                        let block = match aquifer.compute_substance(&ctx, density) {
                                            Some(block) => java_aquifer_substance(block),
                                            None if in_vein_range => ore_veinifier
                                                .compute(&ctx)
                                                .unwrap_or(u32::from(states::STONE.0)),
                                            None => u32::from(states::STONE.0),
                                        };
                                        if block != u32::from(states::AIR.0) {
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


impl JavaGenerator {
    /// Structure starts that originate in chunk (`x`, `z`) (Java's
    /// `structure_starts` status), for tools and parity tests.
    pub fn structure_starts(
        &self,
        x: i32,
        z: i32,
    ) -> unastar_noise::structures::manager::ChunkStarts {
        if !self.options.generate_structures {
            static EMPTY: std::sync::LazyLock<unastar_noise::structures::manager::ChunkStarts> =
                std::sync::LazyLock::new(|| std::sync::Arc::new(Vec::new()));
            return std::sync::Arc::clone(&EMPTY);
        }
        let ctx = self.structures.context(self.seed, self);
        self.structures.manager.starts_in_chunk(&ctx, x, z)
    }
}

impl Generator for JavaGenerator {
    fn seed(&self) -> i64 {
        self.seed
    }

    fn world_bounds(&self) -> std::ops::Range<i32> {
        self.options.dimension.block_bounds()
    }

    fn generate_proto(&self, pos: ChunkPos) -> ProtoChunk {
        self.generate_stage(pos, JavaTerrainStage::Carvers)
    }

    fn decorates(&self) -> bool {
        self.options.generate_structures || self.options.biome_features
    }

    fn decorates_in_order(&self) -> bool {
        matches!(&self.backend, TerrainBackend::NativeJava(_))
    }

    fn decorate(&self, region: &mut DecorationRegion<'_>) {
        super::features::decorate(
            self.seed, self.options.dimension, &self.structures, self, region,
            self.options.generate_structures, self.options.biome_features,
        );
    }

    fn block_classes(&self) -> &'static [u8] {
        &unastar_noise::java_blocks::JAVA_STATE_CLASSES
    }

    fn bedrock_biome(&self, biome: NativeBiome) -> u32 {
        JavaBiome::from_index(biome)
            .expect("Java-style protos store JavaBiome discriminants")
            .data()
            .bedrock_id
    }

    fn find_spawn(&self, chunk_at: &dyn Fn(ChunkPos) -> Chunk) -> SpawnPoint {
        let start = match &self.backend {
            TerrainBackend::NativeJava(noises) => biome_source::spawn_search_start(&noises.java_source()),
            #[cfg(feature = "bedrock-worldgen")]
            TerrainBackend::SharedBedrock(noises) => biome_source::spawn_search_start(noises),
        };
        super::spawn::java_spawn(
            start, chunk_at, &unastar_noise::java_blocks::BEDROCK_STATE_CLASSES,
        )
    }
}

