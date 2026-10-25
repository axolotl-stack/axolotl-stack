//! World module - chunk storage, generation, and ECS integration.

pub mod chunk;
pub mod ecs;
pub mod generator;

pub use chunk::{Chunk, HeightMapType, SUBCHUNK_COUNT, request_mode};
pub use ecs::{ChunkData, ChunkManager, ChunkPosition, ChunkState};
pub use generator::JavaGenerator;

use serde::{Deserialize, Serialize};

/// Chunk coordinate key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChunkPos {
    pub x: i32,
    pub z: i32,
}

impl ChunkPos {
    pub fn new(x: i32, z: i32) -> Self {
        Self { x, z }
    }
}

/// World boundary policy in chunk coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorldBounds {
    /// Infinite world (no bounds checks).
    #[serde(deserialize_with = "crate::config::deserialize_empty_variant")]
    Infinite,
    /// A finite square around (0, 0), inclusive.
    Radius { radius_chunks: u32 },
    /// A finite rectangle, inclusive.
    Rect {
        min_x: i32,
        max_x: i32,
        min_z: i32,
        max_z: i32,
    },
}

impl WorldBounds {
    /// Returns `true` if the chunk position is inside the configured bounds.
    ///
    /// Bounds are applied to *terrain generation* (non-air). Chunks outside bounds still exist,
    /// but are generated as all-air so the client can mesh neighbouring chunks correctly.
    pub fn contains(&self, pos: ChunkPos) -> bool {
        match *self {
            Self::Infinite => true,
            Self::Radius { radius_chunks } => {
                pos.x.unsigned_abs() <= radius_chunks && pos.z.unsigned_abs() <= radius_chunks
            }
            Self::Rect {
                min_x,
                max_x,
                min_z,
                max_z,
            } => (min_x..=max_x).contains(&pos.x) && (min_z..=max_z).contains(&pos.z),
        }
    }
}

/// World generator for chunks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorldGenerator {
    /// Superflat world: classic Minecraft superflat (1 bedrock, 2 dirt, 1 grass at Y=0-3).
    #[serde(deserialize_with = "crate::config::deserialize_empty_variant")]
    SuperFlat,
    /// A 3x3 (or larger) stone platform in an otherwise-void world.
    VoidSpawnPlatform { platform_radius_chunks: u32 },
    /// Java-style terrain (Java Edition 1.21.11 parity target).
    Java { seed: i64 },
}

impl WorldGenerator {
    /// Instantiate the generator this config selects.
    pub fn build(&self) -> std::sync::Arc<dyn generator::Generator> {
        match *self {
            Self::SuperFlat => std::sync::Arc::new(generator::FlatGenerator),
            Self::VoidSpawnPlatform {
                platform_radius_chunks,
            } => std::sync::Arc::new(generator::VoidGenerator {
                platform_radius_chunks,
            }),
            Self::Java { seed } => std::sync::Arc::new(generator::JavaGenerator::new(seed)),
        }
    }

    /// Whether generation is expensive enough to run on the generation pool instead of
    /// inline on the ECS thread.
    pub fn uses_generation_pool(&self) -> bool {
        match self {
            Self::SuperFlat | Self::VoidSpawnPlatform { .. } => false,
            Self::Java { .. } => true,
        }
    }

    /// World spawn chosen by this generator. The search generates chunks, so it runs once
    /// per generator config and process; later calls return the cached point.
    pub fn spawn(&self) -> generator::SpawnPoint {
        use std::collections::HashMap;
        use std::sync::{Arc, LazyLock, Mutex, OnceLock};
        type Spawns = Mutex<HashMap<WorldGenerator, Arc<OnceLock<generator::SpawnPoint>>>>;
        static SPAWNS: LazyLock<Spawns> = LazyLock::new(Default::default);
        let cell = SPAWNS
            .lock()
            .expect("spawn cache lock poisoned")
            .entry(*self)
            .or_default()
            .clone();
        *cell.get_or_init(|| generator::ChunkPipeline::new(self.build()).find_spawn())
    }
}

/// Storage provider for world persistence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum StorageProvider {
    /// LevelDB - standard Bedrock-compatible storage (default).
    #[default]
    LevelDb,
    /// BlazeDB - high-performance with spatial indexing.
    BlazeDb,
}

/// World configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WorldConfig {
    pub dimension: i32,
    pub bounds: WorldBounds,
    pub generator: WorldGenerator,
    /// Storage provider (leveldb or blazedb).
    pub storage_provider: StorageProvider,
    /// BlazeDB cache capacity in chunks (default: 4096).
    pub blazedb_cache_chunks: usize,
    /// Chunk generation worker threads; 0 picks half the available cores (at least 1,
    /// at most 8).
    pub generation_threads: usize,
}

impl Default for WorldConfig {
    fn default() -> Self {
        Self {
            dimension: 0,
            bounds: WorldBounds::Infinite,
            // Java-style terrain; seed 12345 is the seed of the Java reference fixtures.
            generator: WorldGenerator::Java { seed: 12345 },
            storage_provider: StorageProvider::LevelDb,
            blazedb_cache_chunks: 4096,
            generation_threads: 0,
        }
    }
}
