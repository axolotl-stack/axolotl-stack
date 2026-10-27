//! Mutable native chunk history. Unlike independent decoration replay, an origin
//! observes the writes of every previously completed neighboring origin.
//!
//! This is source-world state, not a disposable memo cache: evicting it and
//! regenerating against a later biome/feature history changes native results.

use std::collections::HashMap;

use super::{Generator, ProtoChunk, SLOT_OFFSETS};
use crate::world::ChunkPos;
use crate::world::chunk::Chunk;
use crate::world::generator::region::{DecorationRegion, HeightmapKind};

struct NativeChunk {
    proto: ProtoChunk,
    /// CARVERS commits the final-map update set. WG maps stay frozen thereafter.
    worldgen_heights: Box<[[i16; 256]; 2]>,
    features_complete: bool,
}

impl NativeChunk {
    fn new(generator: &dyn Generator, pos: ChunkPos) -> Self {
        let proto = generator.generate_proto(pos);
        let bounds = generator.world_bounds();
        let classes = generator.block_classes();
        let mut heights = Box::new([[bounds.start as i16; 256]; 2]);
        for z in 0..16u8 {
            for x in 0..16u8 {
                let column = usize::from(z) * 16 + usize::from(x);
                let mut found = [false; 2];
                for y in bounds.clone().rev() {
                    let flags = classes[proto.chunk.get_block(x, y as i16, z) as usize];
                    for (index, kind) in [HeightmapKind::WorldSurfaceWg, HeightmapKind::OceanFloorWg]
                        .into_iter().enumerate()
                    {
                        if !found[index] && kind.counts(flags) {
                            heights[index][column] = (y + 1) as i16;
                            found[index] = true;
                        }
                    }
                    if found == [true; 2] {
                        break;
                    }
                }
            }
        }
        Self { proto, worldgen_heights: heights, features_complete: false }
    }
}

#[derive(Default)]
pub(super) struct NativeSession {
    chunks: HashMap<ChunkPos, NativeChunk>,
}

impl NativeSession {
    pub(super) fn decorate(&mut self, generator: &dyn Generator, origin: ChunkPos) {
        if self.chunks.get(&origin).is_some_and(|chunk| chunk.features_complete) {
            return;
        }
        let mut chunks = SLOT_OFFSETS.map(|(dx, dz)| {
            let pos = ChunkPos::new(origin.x + dx, origin.z + dz);
            self.chunks.remove(&pos).unwrap_or_else(|| NativeChunk::new(generator, pos))
        });
        {
            let protos = chunks.each_mut()
                .map(|chunk| (&mut chunk.proto, &*chunk.worldgen_heights));
            let mut region = DecorationRegion::new_mutable_with_bounds(
                generator.seed(), origin, protos, generator.block_classes(), generator.world_bounds(),
            );
            generator.decorate(&mut region);
        }
        for mut chunk in chunks {
            if chunk.proto.pos == origin {
                chunk.features_complete = true;
            }
            self.chunks.insert(chunk.proto.pos, chunk);
        }
    }

    pub(super) fn snapshot(&self, generator: &dyn Generator, pos: ChunkPos) -> Option<Chunk> {
        let proto = &self.chunks.get(&pos)?.proto;
        let mut chunk = proto.chunk.clone();
        chunk.set_biomes_from_quarts(&proto.biomes, u32::from);
        debug_assert_eq!(chunk.block_bounds(), generator.world_bounds());
        Some(chunk)
    }

    pub(super) fn worldgen_heightmaps(&self, pos: ChunkPos) -> Option<[[i16; 256]; 2]> {
        Some(*self.chunks.get(&pos)?.worldgen_heights)
    }
}
