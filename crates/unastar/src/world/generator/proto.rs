//! Proto chunks: the output of the per-chunk terrain stages, before decoration.
//!
//! The chunk pipeline ([`super::pipeline`]) caches protos so decoration can read
//! neighbours without regenerating them. Native Java biome ties also depend on
//! per-thread query history; a proto is not universally a pure seed/position value.

use std::ops::Range;
use crate::world::ChunkPos;
use crate::world::chunk::{BlockStateSpace, Chunk, MIN_Y, SUBCHUNK_COUNT};

/// Physical buffer height in blocks. Logical dimension bounds may be smaller.
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
///
/// Each thread keeps the last dropped buffer (384 KiB) for its next chunk, so generation
/// workers allocate it once.
pub struct DenseBlocks {
    blocks: Box<[u32]>,
    state_space: BlockStateSpace,
    bounds: Range<i32>,
}

thread_local! {
    static SPARE_BLOCKS: std::cell::Cell<Option<Box<[u32]>>> = const { std::cell::Cell::new(None) };
}

impl DenseBlocks {
    /// Every block set to `block`, in the declared source-native identity domain.
    pub fn filled(block: u32, state_space: BlockStateSpace) -> Self {
        Self::filled_with_bounds(block, state_space, MIN_Y..MIN_Y + WORLD_HEIGHT)
    }

    /// Fixed-size working storage with a dimension's legal build bounds. Physical
    /// padding is air, never generated content; no extra allocation or copy is made.
    pub fn filled_with_bounds(block: u32, state_space: BlockStateSpace, bounds: Range<i32>) -> Self {
        assert_storage_bounds(&bounds);
        let blocks = match SPARE_BLOCKS.take() {
            Some(mut blocks) => {
                blocks.fill(block);
                blocks
            }
            None => vec![block; BLOCKS_PER_CHUNK].into_boxed_slice(),
        };
        let mut result = Self { blocks, state_space, bounds };
        let first = (result.bounds.start - MIN_Y) as usize * 256;
        let end = (result.bounds.end - MIN_Y) as usize * 256;
        result.blocks[..first].fill(state_space.air());
        result.blocks[end..].fill(state_space.air());
        result
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

    /// Source-native state at local x/z and absolute y; air outside logical build bounds.
    #[inline(always)]
    pub fn get_block(&self, x: u8, y: i16, z: u8) -> u32 {
        if !self.bounds.contains(&i32::from(y)) {
            return self.state_space.air();
        }
        self.blocks[Self::index(x, y, z)]
    }

    /// Set a state in this buffer's identity domain; ignore writes outside build bounds.
    #[inline(always)]
    pub fn set_block(&mut self, x: u8, y: i16, z: u8, block: u32) {
        if !self.bounds.contains(&i32::from(y)) {
            return;
        }
        self.blocks[Self::index(x, y, z)] = block;
    }

    /// Logical build/storage blocks, independent of the fixed physical working buffer.
    pub fn block_bounds(&self) -> Range<i32> {
        self.bounds.clone()
    }

    /// Pack into a chunk (paletted sub-chunks and the light height map).
    pub fn to_chunk(&self, pos: ChunkPos) -> Chunk {
        Chunk::from_dense_with_bounds(pos.x, pos.z, &self.blocks, self.state_space, self.bounds.clone())
    }

    /// Explicit edition boundary for consumers applying Bedrock surface/carver rules.
    pub fn convert_to_bedrock(&mut self) {
        if self.state_space == BlockStateSpace::Java {
            for state in &mut self.blocks {
                *state = self.state_space.to_bedrock(*state);
            }
            self.state_space = BlockStateSpace::Bedrock;
        }
    }
}

impl Drop for DenseBlocks {
    fn drop(&mut self) {
        SPARE_BLOCKS.set(Some(std::mem::take(&mut self.blocks)));
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
    bounds: Range<i32>,
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
            bounds: MIN_Y..MIN_Y + WORLD_HEIGHT,
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
        let quart_y = (qy as i32 + (MIN_Y >> 2))
            .clamp(self.bounds.start >> 2, (self.bounds.end - 1) >> 2);
        let qy = (quart_y - (MIN_Y >> 2)) as usize;
        self.ids[Self::index(qx, qy, qz)]
    }

    /// Set the biome of a local quart cell.
    #[inline]
    pub fn set(&mut self, qx: usize, qy: usize, qz: usize, biome: NativeBiome) {
        let y = MIN_Y + qy as i32 * 4;
        if !self.bounds.contains(&y) {
            return;
        }
        self.ids[Self::index(qx, qy, qz)] = biome;
    }

    /// Biome of the quart cell containing a block (local x/z, absolute y clamped to the world).
    #[inline]
    pub fn at_block(&self, local_x: u8, y: i32, local_z: u8) -> NativeBiome {
        let y = y.clamp(self.bounds.start, self.bounds.end - 1);
        let qy = ((y - MIN_Y) >> 2) as usize;
        self.get(local_x as usize >> 2, qy, local_z as usize >> 2)
    }

    /// Clamp biome reads and writes to the actual dimension's storage quart range.
    /// The flat array keeps its physical `MIN_Y` indexing for chunk conversion.
    pub fn set_block_bounds(&mut self, bounds: Range<i32>) {
        assert_storage_bounds(&bounds);
        self.bounds = bounds;
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
    /// Blocks and sparse immutable actors; block ids remain in the chunk's native domain.
    pub chunk: Chunk,
    /// Generator-native 3D biomes.
    pub biomes: QuartBiomes,
}

impl ProtoChunk {
    /// An empty (all-air) proto with uniform biomes.
    pub fn empty(pos: ChunkPos, biome: NativeBiome) -> Self {
        Self::empty_in_space(pos, biome, BlockStateSpace::Bedrock)
    }

    /// Empty proto in an explicit state domain; Java fixtures must not use Bedrock air.
    pub fn empty_in_space(pos: ChunkPos, biome: NativeBiome, state_space: BlockStateSpace) -> Self {
        Self {
            pos,
            chunk: Chunk::new_in_space(pos.x, pos.z, state_space),
            biomes: QuartBiomes::filled(biome),
        }
    }
}

/// All logical bounds fit the shared physical buffer and complete biome sections.
fn assert_storage_bounds(bounds: &Range<i32>) {
    assert!(
        bounds.start >= MIN_Y
            && bounds.end <= MIN_Y + WORLD_HEIGHT
            && bounds.start < bounds.end
            && bounds.start % 16 == 0
            && bounds.end % 16 == 0,
        "logical dimension bounds {bounds:?} exceed physical chunk storage"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::block_entity::BlockEntity;
    use crate::world::generator::JavaDimension;
    use unastar_noise::features::{java_blocks::states, eval::BlockEntityData};

    #[test]
    fn native_storage_keeps_upper_half_and_rejects_padding_through_persistence() {
        let air = u32::from(states::AIR.0);
        let chest = u32::from(states::CHEST.0);
        for dimension in [JavaDimension::Nether, JavaDimension::End] {
            let bounds = dimension.block_bounds();
            let mut dense = DenseBlocks::filled_with_bounds(air, BlockStateSpace::Java, bounds.clone());
            for y in [-64, -1, 256, 319] {
                dense.set_block(0, y, 0, chest);
                assert_eq!(dense.get_block(0, y, 0), air);
            }
            for y in [0, 127, 128, 255] {
                dense.set_block(0, y, 0, chest);
            }
            let mut chunk = dense.to_chunk(ChunkPos::new(0, 0));
            assert_eq!(chunk.block_bounds(), 0..256);
            assert_eq!(chunk.height_map().at(0, 0), 256);
            for y in [-64, -1, 256, 319] {
                assert_eq!(chunk.get_block(0, y, 0), air);
                assert_eq!(chunk.set_block(0, y, 0, chest), None);
                assert!(!chunk.set_block_entity(BlockEntity::generated_java(
                    [0, i32::from(y), 0], "minecraft:chest",
                    BlockEntityData::loot("minecraft:chests/simple_dungeon", 7),
                )));
            }
            assert!(chunk.set_block_entity(BlockEntity::generated_java(
                [0, 255, 0], "minecraft:chest", BlockEntityData::loot("minecraft:chests/simple_dungeon", 9),
            )));
            let mut biomes = QuartBiomes::filled(1);
            biomes.set(0, ((0 - MIN_Y) >> 2) as usize, 0, 2);
            biomes.set(0, ((255 - MIN_Y) >> 2) as usize, 0, 3);
            biomes.set_block_bounds(bounds.clone());
            biomes.set(0, 0, 0, 4); // A write into physical padding is denied.
            assert_eq!(biomes.at_block(0, i32::MIN, 0), 2);
            assert_eq!(biomes.at_block(0, i32::MAX, 0), 3);
            assert_eq!(biomes.get(0, 0, 0), 2);
            chunk.set_biomes_from_quarts(&biomes, u32::from);
            assert_eq!(chunk.biome_at(0, -64, 0), Some(2));
            assert_eq!(chunk.biome_at(0, 319, 0), Some(3));
            // Storage above the 128-block noise volume survives the real final
            // edition boundary and persisted subchunk codec, without rebaselining.
            for y in [0, 127, 128, 255] {
                assert_eq!(chunk.get_block(0, y, 0), chest);
            }
            chunk.convert_to_bedrock().unwrap();
            assert_eq!(chunk.block_bounds(), bounds);
            assert_eq!(chunk.block_entities()[0].position(), [0, 255, 0]);
            assert!(chunk.encode_subchunk_disk(-1).is_none());
            assert!(chunk.encode_subchunk_disk(16).is_none());
            let mut loaded = Chunk::new_with_bounds(0, 0, BlockStateSpace::Bedrock, bounds);
            for section in [0, 7, 8, 15] {
                let encoded = chunk.encode_subchunk_disk(section).unwrap();
                loaded.decode_subchunk(section, &encoded).unwrap();
            }
            for y in [0, 127, 128, 255] {
                assert_eq!(loaded.get_block(0, y, 0), BlockStateSpace::Java.to_bedrock(chest));
            }
            assert_eq!(loaded.height_map().at(0, 0), 256);
            assert!(loaded.set_block(0, -1, 0, *crate::world::chunk::blocks::STONE).is_none());
        }
    }

    #[test]
    fn adopting_native_bounds_removes_padding_actors_and_clamps_bulk_writes() {
        let air = u32::from(states::AIR.0);
        let chest = u32::from(states::CHEST.0);
        let mut chunk = Chunk::new_in_space(0, 0, BlockStateSpace::Java);
        for y in [-1, 128, 255, 319] {
            chunk.set_block(0, y, 0, chest);
            assert!(chunk.set_block_entity(BlockEntity::generated_java(
                [0, i32::from(y), 0], "minecraft:chest",
                BlockEntityData::loot("minecraft:chests/simple_dungeon", i64::from(y)),
            )));
        }
        chunk.set_block_bounds(JavaDimension::Nether.block_bounds());
        assert_eq!(chunk.height_map().at(0, 0), 256);
        assert_eq!(chunk.block_entities().iter().map(BlockEntity::position).collect::<Vec<_>>(),
            vec![[0, 128, 0], [0, 255, 0]]);
        assert_eq!(chunk.get_block(0, -1, 0), air);
        assert_eq!(chunk.get_block(0, 319, 0), air);
        chunk.set_block(0, 255, 0, air);
        assert_eq!(chunk.height_map().at(0, 0), 129);
        chunk.set_block(0, 128, 0, air);
        assert_eq!(chunk.height_map().at(0, 0), 0);
        chunk.fill_subchunk_solid(3, chest); // Physical section below y=0.
        chunk.fill_subchunk_solid(20, chest); // Physical section above y=255.
        assert_eq!(chunk.height_map().at(0, 0), 0);
        chunk.fill_floor(400, chest);
        assert_eq!(chunk.height_map().at(0, 0), 256);
        assert_eq!(chunk.get_block(0, 255, 0), chest);
        assert_eq!(chunk.get_block(0, -1, 0), air);
        assert_eq!(chunk.get_block(0, 256, 0), air);
    }
}
