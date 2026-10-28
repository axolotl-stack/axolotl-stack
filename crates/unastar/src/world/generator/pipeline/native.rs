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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::block_entity::BlockEntity;
    use crate::world::chunk::BlockStateSpace;
    use crate::world::generator::{ChunkPipeline, SpawnPoint};
    use std::sync::{Arc, LazyLock};
    use unastar_noise::features::JavaBlock;

    static FULL_ACTORS: LazyLock<serde_json::Value> = LazyLock::new(|| {
        serde_json::from_str(include_str!(
            "../../../../tests/fixtures/native_pending_full_actor_save_boundaries_1_21_11.json"
        )).unwrap()
    });

    /// Recreate the native probe's controlled input blocks and pending markers,
    /// never its expected constructor/save payloads.
    struct PendingActorGrid;

    impl Generator for PendingActorGrid {
        fn seed(&self) -> i64 { 0 }
        fn generate_proto(&self, pos: ChunkPos) -> ProtoChunk {
            let mut proto = ProtoChunk::empty_in_space(pos, 0, BlockStateSpace::Java);
            if pos == ChunkPos::new(0, 0) {
                for row in FULL_ACTORS["records"].as_array().unwrap() {
                    let block = JavaBlock::by_name(row["block"].as_str().unwrap()).unwrap();
                    let position = std::array::from_fn(|index| row["position"][index].as_i64().unwrap() as i32);
                    let [x, y, z] = position;
                    proto.chunk.set_block(x as u8, y as i16, z as u8, u32::from(block.default_state().0));
                    assert!(proto.chunk.set_native_block_entity(BlockEntity::pending_java(position, block.name())));
                }
            }
            proto
        }
        fn decorates(&self) -> bool { true }
        fn decorates_in_order(&self) -> bool { true }
        fn decorate(&self, _: &mut DecorationRegion<'_>) {}
        fn block_classes(&self) -> &'static [u8] { &unastar_noise::java_blocks::JAVA_STATE_CLASSES }
        fn bedrock_biome(&self, biome: super::super::NativeBiome) -> u32 { u32::from(biome) }
        fn find_spawn(&self, _: &dyn Fn(ChunkPos) -> Chunk) -> SpawnPoint {
            SpawnPoint { x: 0, y: 201, z: 0 }
        }
    }

    #[test]
    fn finished_native_actor_saves_match_actual_full_promotion() {
        let pipeline = ChunkPipeline::new(Arc::new(PendingActorGrid));
        let pos = ChunkPos::new(0, 0);
        pipeline.decorate_origin(pos);
        let proto = pipeline.native_snapshot(pos).unwrap();
        for row in FULL_ACTORS["records"].as_array().unwrap() {
            let position: [i32; 3] = std::array::from_fn(|index| row["position"][index].as_i64().unwrap() as i32);
            let actor = proto.block_entities().iter().find(|actor| actor.position() == position).unwrap();
            assert_eq!(actor.canonical_java_nbt().unwrap(), row["proto_saved"]);
        }
        let full = pipeline.generate_native(pos);
        for row in FULL_ACTORS["records"].as_array().unwrap() {
            let position: [i32; 3] = std::array::from_fn(|index| row["position"][index].as_i64().unwrap() as i32);
            let actor = full.block_entities().iter().find(|actor| actor.position() == position);
            if row["full_saved"].is_null() {
                assert!(actor.is_none(), "native FULL removed {}", row["block"]);
            } else {
                assert_eq!(actor.unwrap().canonical_java_level_nbt().unwrap(), row["full_saved"], "{}", row["block"]);
            }
        }
    }
}
