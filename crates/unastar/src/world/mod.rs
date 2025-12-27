//! World module - chunk storage, generation, and ECS integration.

pub mod chunk;
pub mod ecs;

pub use chunk::{Chunk, HeightMapType, SUBCHUNK_COUNT, request_mode};
pub use ecs::{ChunkData, ChunkManager, ChunkPosition, ChunkState};

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
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum WorldBounds {
    /// Infinite world (no bounds checks).
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorldGenerator {
    /// Flat world: a solid 16-block-thick stone layer at Y=0..15.
    FlatStone,
    /// A 3x3 (or larger) stone platform in an otherwise-void world.
    VoidSpawnPlatform { platform_radius_chunks: u32 },
}

/// World configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WorldConfig {
    pub dimension: i32,
    pub bounds: WorldBounds,
    pub generator: WorldGenerator,
}

impl Default for WorldConfig {
    fn default() -> Self {
        Self {
            dimension: 0,
            bounds: WorldBounds::Infinite,
            generator: WorldGenerator::VoidSpawnPlatform {
                platform_radius_chunks: 1,
            },
        }
    }
}
