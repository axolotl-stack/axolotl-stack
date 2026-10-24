//! Proto chunks: the output of the per-chunk terrain stages, before decoration.
//!
//! A proto chunk is a pure function of `(seed, chunk position)`. The chunk pipeline
//! ([`super::pipeline`]) caches protos so the 3x3 decoration stage can read a chunk's
//! neighbours without regenerating them.

use crate::world::ChunkPos;
use crate::world::chunk::{Chunk, MIN_Y, SUBCHUNK_COUNT};

/// World height in blocks (`MIN_Y..MIN_Y + WORLD_HEIGHT`).
pub const WORLD_HEIGHT: i32 = SUBCHUNK_COUNT as i32 * 16;

/// Quart (4x4x4 block) cells per chunk column.
pub const QUARTS_Y: usize = (WORLD_HEIGHT / 4) as usize;

/// Quart cells per chunk.
pub const QUARTS_PER_CHUNK: usize = 4 * 4 * QUARTS_Y;

/// Generator-native biome id.
///
/// Its meaning belongs to the generator that wrote it: the Java-style generator stores
/// `unastar_noise::JavaBiome` discriminants, the Bedrock-style generator stores Bedrock
/// biome ids. [`super::Generator::bedrock_biome`] converts it for the network and disk.
pub type NativeBiome = u16;

/// 3D biomes of one chunk at quart resolution, like Java's biome storage.
#[derive(Clone, PartialEq, Eq)]
pub struct QuartBiomes {
    ids: Box<[NativeBiome; QUARTS_PER_CHUNK]>,
}

impl std::fmt::Debug for QuartBiomes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QuartBiomes").finish_non_exhaustive()
    }
}

impl QuartBiomes {
    /// Every quart set to `biome`.
    pub fn filled(biome: NativeBiome) -> Self {
        Self {
            ids: Box::new([biome; QUARTS_PER_CHUNK]),
        }
    }

    #[inline]
    fn index(qx: usize, qy: usize, qz: usize) -> usize {
        debug_assert!(qx < 4 && qz < 4 && qy < QUARTS_Y);
        (qy * 4 + qz) * 4 + qx
    }

    /// Biome of the local quart cell (`qx`, `qz` in `0..4`, `qy` in `0..QUARTS_Y` from `MIN_Y`).
    #[inline]
    pub fn get(&self, qx: usize, qy: usize, qz: usize) -> NativeBiome {
        self.ids[Self::index(qx, qy, qz)]
    }

    /// Set the biome of a local quart cell.
    #[inline]
    pub fn set(&mut self, qx: usize, qy: usize, qz: usize, biome: NativeBiome) {
        self.ids[Self::index(qx, qy, qz)] = biome;
    }

    /// Biome of the quart cell containing a block (local x/z, absolute y clamped to the world).
    #[inline]
    pub fn at_block(&self, local_x: u8, y: i32, local_z: u8) -> NativeBiome {
        let qy = ((y - MIN_Y).clamp(0, WORLD_HEIGHT - 1) >> 2) as usize;
        self.get(local_x as usize >> 2, qy, local_z as usize >> 2)
    }

    /// Raw ids in `(qy, qz, qx)` order.
    pub fn as_slice(&self) -> &[NativeBiome] {
        &self.ids[..]
    }
}

/// Output of the terrain stages for one chunk: blocks (noise, aquifers, ore veins,
/// surface, carvers) and 3D biomes.
#[derive(Clone, Debug)]
pub struct ProtoChunk {
    /// Chunk position.
    pub pos: ChunkPos,
    /// Blocks, with Bedrock runtime ids.
    pub chunk: Chunk,
    /// Generator-native 3D biomes.
    pub biomes: QuartBiomes,
}

impl ProtoChunk {
    /// An empty (all-air) proto with uniform biomes.
    pub fn empty(pos: ChunkPos, biome: NativeBiome) -> Self {
        Self {
            pos,
            chunk: Chunk::new(pos.x, pos.z),
            biomes: QuartBiomes::filled(biome),
        }
    }
}
