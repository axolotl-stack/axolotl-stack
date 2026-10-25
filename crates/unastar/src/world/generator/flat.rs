//! Flat and void generators.

use super::pipeline::{Generator, SpawnPoint};
use super::proto::{NativeBiome, ProtoChunk};
use super::region::DecorationRegion;
use crate::world::ChunkPos;
use crate::world::chunk::{Chunk, DEFAULT_BIOME_ID, blocks};

/// Classic superflat: bedrock at y=0, dirt at y=1-2, grass at y=3, plains everywhere.
#[derive(Debug, Default, Clone, Copy)]
pub struct FlatGenerator;

impl Generator for FlatGenerator {
    fn seed(&self) -> i64 {
        0
    }

    fn generate_proto(&self, pos: ChunkPos) -> ProtoChunk {
        let mut proto = ProtoChunk::empty(pos, DEFAULT_BIOME_ID as NativeBiome);
        let chunk = &mut proto.chunk;
        // Lower layers last so they overwrite the grass fill.
        chunk.fill_floor(4, *blocks::GRASS_BLOCK);
        chunk.fill_floor(3, *blocks::DIRT);
        chunk.fill_floor(1, *blocks::BEDROCK);
        proto
    }

    fn decorates(&self) -> bool {
        false
    }

    fn decorate(&self, _region: &mut DecorationRegion<'_>) {}

    fn block_classes(&self) -> &'static [u8] {
        &unastar_noise::java_blocks::BEDROCK_STATE_CLASSES
    }

    fn bedrock_biome(&self, biome: NativeBiome) -> u32 {
        biome as u32
    }

    fn find_spawn(&self, _chunk_at: &dyn Fn(ChunkPos) -> Chunk) -> SpawnPoint {
        SpawnPoint { x: 0, y: 4, z: 0 }
    }
}

/// A stone platform (sub-chunk y=0..16) within `platform_radius_chunks` of the origin
/// chunk; air elsewhere.
#[derive(Debug, Clone, Copy)]
pub struct VoidGenerator {
    /// Platform radius in chunks (Chebyshev distance from chunk 0,0).
    pub platform_radius_chunks: u32,
}

impl Generator for VoidGenerator {
    fn seed(&self) -> i64 {
        0
    }

    fn generate_proto(&self, pos: ChunkPos) -> ProtoChunk {
        let mut proto = ProtoChunk::empty(pos, DEFAULT_BIOME_ID as NativeBiome);
        if pos.x.unsigned_abs() <= self.platform_radius_chunks
            && pos.z.unsigned_abs() <= self.platform_radius_chunks
        {
            proto.chunk.fill_subchunk_solid(4, *blocks::STONE);
        }
        proto
    }

    fn decorates(&self) -> bool {
        false
    }

    fn decorate(&self, _region: &mut DecorationRegion<'_>) {}

    fn block_classes(&self) -> &'static [u8] {
        &unastar_noise::java_blocks::BEDROCK_STATE_CLASSES
    }

    fn bedrock_biome(&self, biome: NativeBiome) -> u32 {
        biome as u32
    }

    fn find_spawn(&self, _chunk_at: &dyn Fn(ChunkPos) -> Chunk) -> SpawnPoint {
        SpawnPoint { x: 0, y: 16, z: 0 }
    }
}
