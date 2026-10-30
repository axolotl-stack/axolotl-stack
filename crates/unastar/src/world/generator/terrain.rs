//! Java-style generator: 3D terrain from the AOT-compiled noise router, aquifers, ore
//! veins, 3D biomes, surface rules and carvers.
//!
//! This module implements Java Edition-style terrain generation with:
//! - Density functions for 3D terrain shaping (overhangs, caves, etc.)
//! - Cell-based interpolation for performance
//! - Aquifer system for underground water/lava pockets
//! - Surface rules for biome-based block placement
//! - Cave and canyon carvers

mod columns;
mod native;
mod source;

use std::collections::HashSet;
use std::sync::Mutex;

use super::JavaDimension;
#[cfg(feature = "bedrock-worldgen")]
use super::aquifer::{NoiseBasedAquifer, OverworldFluidPicker};
use super::biome_source;
use super::density::{NativeNoiseRegistry, NoiseRegistry};
#[cfg(feature = "bedrock-worldgen")]
use super::density::{ColumnContextGrid, FlatCacheGrid, JavaNoiseSource};
use super::pipeline::{Generator, SpawnPoint};
use super::proto::{NativeBiome, ProtoChunk, QuartBiomes};
use super::region::DecorationRegion;
use super::surface::SurfaceSystem;
use super::xoroshiro::PositionalRandomFactory;
use crate::world::ChunkPos;
#[cfg(feature = "bedrock-worldgen")]
use crate::world::chunk::BlockStateSpace;
use crate::world::chunk::{Chunk, blocks};
#[cfg(feature = "bedrock-worldgen")]
use crate::world::generator::proto::DenseBlocks;
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
    /// Java-exact overworld noise without Java's structures: BDS terrain
    /// heights and noise caves follow the same arithmetic (see "Java-exact
    /// Bedrock terrain" in the codegen README).
    #[cfg(feature = "bedrock-worldgen")]
    SharedBedrock(NativeNoiseRegistry),
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
    /// History-independent native biome fills (see `native_biomes`). A chunk's
    /// own terrain and the surfaces of its eight neighbours each fill its
    /// biomes.
    biome_fills: super::pipeline::ChunkCache<Option<(QuartBiomes, biome_source::Replay)>>,
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
                NoiseRegistry::with_router_settings(
                    seed,
                    settings::LEGACY_RANDOM_SOURCE,
                    [
                        params.xz_scale,
                        params.y_scale,
                        params.xz_factor,
                        params.y_factor,
                        params.smear_scale_multiplier,
                    ],
                )
            }};
        }
        let noises = match options.dimension {
            JavaDimension::Overworld => native_registry!(unastar_noise::overworld_settings),
            JavaDimension::Nether => native_registry!(unastar_noise::nether_settings),
            JavaDimension::End => native_registry!(unastar_noise::end_settings),
        };
        Self::with_backend(seed, options, TerrainBackend::NativeJava(noises))
    }

    /// The Bedrock owner's shared terrain core: Java-exact overworld noise,
    /// aquifers and ore veins, without Java's structure starts or beardifier.
    #[cfg(feature = "bedrock-worldgen")]
    pub fn for_bedrock_shared_terrain(seed: i64) -> Self {
        let params = overworld_settings::BLENDED_NOISE;
        let noises = NoiseRegistry::with_router_settings(seed, overworld_settings::LEGACY_RANDOM_SOURCE, [
            params.xz_scale, params.y_scale, params.xz_factor,
            params.y_factor, params.smear_scale_multiplier,
        ]);
        let options = JavaGenerationOptions { generate_structures: false, ..JavaGenerationOptions::default() };
        Self::with_backend(seed, options, TerrainBackend::SharedBedrock(noises))
    }

    fn with_backend(seed: i64, options: JavaGenerationOptions, backend: TerrainBackend) -> Self {
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
            biome_fills: super::pipeline::ChunkCache::new(super::pipeline::DEFAULT_PROTO_CACHE),
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
    fn bedrock_source(&self) -> JavaNoiseSource<'_> {
        match &self.backend {
            TerrainBackend::SharedBedrock(noises) => noises.java_source(),
            TerrainBackend::NativeJava(_) => panic!("Bedrock sampling requires the Bedrock backend"),
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
                if stage == JavaTerrainStage::StructureReferences
                    && self.options.generate_structures
                {
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
            "the shared Bedrock backend only exposes biome and noise stages");
        if stage == JavaTerrainStage::Biomes {
            let source = self.bedrock_source();
            let flat = FlatCacheGrid::new_native(pos.x, pos.z, &source);
            let columns = ColumnContextGrid::new_native(pos.x, pos.z, &source, &flat);
            return ProtoChunk {
                pos,
                chunk: Chunk::new_with_bounds(
                    pos.x,
                    pos.z,
                    BlockStateSpace::Java,
                    self.options.dimension.block_bounds(),
                ),
                biomes: biome_source::fill_overworld_native(&source, pos, &flat, &columns).biomes,
            };
        }
        let (dense, biomes) = self.generate_noise_with(pos, |source, pos, flat, columns| {
            biome_source::fill_overworld_native(source, pos, flat, columns).biomes
        });
        ProtoChunk { pos, chunk: dense.to_chunk(pos), biomes }
    }

    /// Share the density stage and climate caches without imposing Java's
    /// stateful biome-search policy on another edition or on base-height queries.
    #[cfg(feature = "bedrock-worldgen")]
    pub(crate) fn generate_noise_with<T>(
        &self,
        pos: ChunkPos,
        fill: impl FnOnce(&JavaNoiseSource<'_>, ChunkPos, &FlatCacheGrid, &ColumnContextGrid) -> T,
    ) -> (DenseBlocks, T) {
        let _noise = profile::scope(Stage::Noise);
        let source = self.bedrock_source();
        let flat = FlatCacheGrid::new_native(pos.x, pos.z, &source);
        let columns = ColumnContextGrid::new_native(pos.x, pos.z, &source, &flat);
        let biomes = fill(&source, pos, &flat, &columns);
        let mut aquifer = NoiseBasedAquifer::new(
            pos.x, pos.z, overworld_settings::MIN_Y, overworld_settings::HEIGHT, &source,
            &flat, &columns, self.seed, OverworldFluidPicker::new(Self::SEA_LEVEL),
        );
        let dense = self.overworld_native_cells(pos, &source, &flat, &columns, &mut aquifer);
        (dense, biomes)
    }

    /// Native Java biome at the quart containing an absolute block position.
    pub fn sample_biome(&self, x: i32, y: i32, z: i32) -> NativeBiome {
        let source = match &self.backend {
            TerrainBackend::NativeJava(noises) => noises.java_source(),
            #[cfg(feature = "bedrock-worldgen")]
            TerrainBackend::SharedBedrock(noises) => noises.java_source(),
        };
        biome_source::sample_biome_native(self.options.dimension, &source, x >> 2, y >> 2, z >> 2)
            as NativeBiome
    }

    /// Climate target for a foreign edition's independently owned biome lookup.
    #[cfg(feature = "bedrock-worldgen")]
    pub(crate) fn sample_climate(&self, x: i32, y: i32, z: i32) -> [i64; 7] {
        biome_source::overworld_target_native(&self.bedrock_source(), x >> 2, y >> 2, z >> 2)
    }

    /// Shared Bedrock-backend motion-blocking heights, cached by the Bedrock
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
            self.seed,
            self.options.dimension,
            &self.structures,
            self,
            region,
            self.options.generate_structures,
            self.options.biome_features,
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

    fn find_spawn(
        &self,
        chunk_at: &dyn Fn(ChunkPos) -> Chunk,
        bounds: crate::world::WorldBounds,
    ) -> SpawnPoint {
        let start = match &self.backend {
            TerrainBackend::NativeJava(noises) => {
                biome_source::spawn_search_start(&noises.java_source())
            }
            #[cfg(feature = "bedrock-worldgen")]
            TerrainBackend::SharedBedrock(noises) => biome_source::spawn_search_start(&noises.java_source()),
        };
        super::spawn::java_spawn(
            start,
            bounds,
            chunk_at,
            &unastar_noise::java_blocks::BEDROCK_STATE_CLASSES,
        )
    }
}
