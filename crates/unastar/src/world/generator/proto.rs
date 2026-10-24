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

/// Blocks per chunk.
pub const BLOCKS_PER_CHUNK: usize = 16 * 16 * WORLD_HEIGHT as usize;

/// A chunk's blocks as one flat array, the working buffer of the terrain stages.
///
/// Noise fill, surface and carvers touch every column top to bottom; plain indexing here
/// avoids the paletted storage's per-block palette search and repacking. The buffer is
/// packed into a [`Chunk`] once, when the stages are done. Index:
/// `((y - MIN_Y) * 16 + z) * 16 + x`.
pub struct DenseBlocks {
    blocks: Box<[u32]>,
}

impl DenseBlocks {
    /// Every block set to `block`.
    pub fn filled(block: u32) -> Self {
        Self {
            blocks: vec![block; BLOCKS_PER_CHUNK].into_boxed_slice(),
        }
    }

    #[inline(always)]
    fn index(x: u8, y: i16, z: u8) -> usize {
        debug_assert!(x < 16 && z < 16);
        let y = y as i32 - MIN_Y;
        assert!(
            (0..WORLD_HEIGHT).contains(&y),
            "block y {} outside the world",
            y + MIN_Y
        );
        ((y as usize * 16) + z as usize) * 16 + x as usize
    }

    /// Block at local x/z and absolute y (panics outside the world height).
    #[inline(always)]
    pub fn get_block(&self, x: u8, y: i16, z: u8) -> u32 {
        self.blocks[Self::index(x, y, z)]
    }

    /// Set a block at local x/z and absolute y (panics outside the world height).
    #[inline(always)]
    pub fn set_block(&mut self, x: u8, y: i16, z: u8, block: u32) {
        self.blocks[Self::index(x, y, z)] = block;
    }

    /// Pack into a chunk (paletted sub-chunks and the light height map).
    pub fn to_chunk(&self, pos: ChunkPos, air: u32) -> Chunk {
        Chunk::from_dense(pos.x, pos.z, &self.blocks, air)
    }
}

/// Read/write access to a chunk's blocks by local x/z and absolute y, shared by the
/// paletted [`Chunk`] and the [`DenseBlocks`] working buffer.
pub trait BlockAccess {
    fn get_block(&self, x: u8, y: i16, z: u8) -> u32;
    fn set_block(&mut self, x: u8, y: i16, z: u8, block: u32);
}

impl BlockAccess for DenseBlocks {
    #[inline(always)]
    fn get_block(&self, x: u8, y: i16, z: u8) -> u32 {
        DenseBlocks::get_block(self, x, y, z)
    }

    #[inline(always)]
    fn set_block(&mut self, x: u8, y: i16, z: u8, block: u32) {
        DenseBlocks::set_block(self, x, y, z, block);
    }
}

impl BlockAccess for Chunk {
    #[inline]
    fn get_block(&self, x: u8, y: i16, z: u8) -> u32 {
        Chunk::get_block(self, x, y, z)
    }

    #[inline]
    fn set_block(&mut self, x: u8, y: i16, z: u8, block: u32) {
        Chunk::set_block(self, x, y, z, block);
    }
}

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
