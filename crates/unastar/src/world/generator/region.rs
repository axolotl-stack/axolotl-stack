//! The 3x3 decoration region.
//!
//! Decoration (features and structure pieces) of an origin chunk runs on a
//! [`DecorationRegion`]: it reads the proto chunks of the origin and its 8 neighbours plus
//! its own earlier writes, and writes anywhere in that 3x3 area. Writes are recorded per
//! target chunk; the pipeline applies, to each chunk, the writes of the 9 origins around it
//! in a fixed order. That keeps every finished chunk a pure function of `(seed, position)`
//! regardless of request order, at the cost of one documented difference from Java:
//! decoration does not see a neighbour origin's decoration (Java's result depends on which
//! neighbour happened to be decorated first).

use std::cell::OnceCell;

use unastar_noise::block_class;

use super::proto::{NativeBiome, ProtoChunk, QUARTS_Y, WORLD_HEIGHT};
use crate::world::ChunkPos;
use crate::world::block_entity::BlockEntity;
use crate::world::chunk::{Chunk, MIN_Y, blocks};

/// Heightmap kinds, with Java's predicates over [`block_class`] flags.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HeightmapKind {
    /// Highest non-air block during generation.
    WorldSurfaceWg,
    /// Highest motion-blocking block during generation.
    OceanFloorWg,
    /// Highest non-air block.
    WorldSurface,
    /// Highest motion-blocking block.
    OceanFloor,
    /// Highest motion-blocking or fluid block.
    MotionBlocking,
    /// Highest motion-blocking or fluid block that is not leaves.
    MotionBlockingNoLeaves,
}

impl HeightmapKind {
    /// All kinds, in storage order.
    pub const ALL: [HeightmapKind; 6] = [
        HeightmapKind::WorldSurfaceWg,
        HeightmapKind::OceanFloorWg,
        HeightmapKind::WorldSurface,
        HeightmapKind::OceanFloor,
        HeightmapKind::MotionBlocking,
        HeightmapKind::MotionBlockingNoLeaves,
    ];

    /// Whether a block with these class flags is counted by this heightmap.
    #[inline]
    pub fn counts(self, flags: u8) -> bool {
        match self {
            Self::WorldSurfaceWg | Self::WorldSurface => flags & block_class::AIR == 0,
            Self::OceanFloorWg | Self::OceanFloor => flags & block_class::BLOCKS_MOTION != 0,
            Self::MotionBlocking => flags & (block_class::BLOCKS_MOTION | block_class::FLUID) != 0,
            Self::MotionBlockingNoLeaves => {
                flags & (block_class::BLOCKS_MOTION | block_class::FLUID) != 0
                    && flags & block_class::LEAVES == 0
            }
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// What decoration code may do with the world around its origin chunk.
///
/// Coordinates are absolute block coordinates; quart coordinates are block coordinates
/// shifted right by 2. Reads and writes are limited to the 3x3 chunks around the origin.
pub trait WorldGenRegion {
    /// World seed.
    fn seed(&self) -> i64;
    /// Chunk being decorated.
    fn origin(&self) -> ChunkPos;
    /// Lowest block y.
    fn min_y(&self) -> i32 {
        MIN_Y
    }
    /// World height in blocks.
    fn height(&self) -> i32 {
        WORLD_HEIGHT
    }
    /// Block runtime id; air outside the height range. Panics outside the 3x3 area.
    fn get_block(&self, x: i32, y: i32, z: i32) -> u32;
    /// Set a block; returns `false` (and writes nothing) outside the 3x3 area or height range.
    fn set_block(&mut self, x: i32, y: i32, z: i32, block: u32) -> bool;
    /// Retain an actor at an absolute position; reject positions outside the region/height.
    fn set_block_entity(&mut self, entity: BlockEntity) -> bool;
    /// First y above the highest block counted by `kind` in the column (`min_y` if none).
    fn heightmap(&self, kind: HeightmapKind, x: i32, z: i32) -> i32;
    /// Generator-native biome at an absolute quart position (y clamped to the world).
    fn biome_at(&self, quart_x: i32, quart_y: i32, quart_z: i32) -> NativeBiome;
}

/// One block write recorded by a decoration run, in the target chunk's local coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockWrite {
    /// Local x (0..16).
    pub x: u8,
    /// Local z (0..16).
    pub z: u8,
    /// Absolute y.
    pub y: i16,
    /// Bedrock runtime id.
    pub block: u32,
}

/// Index of a chunk in a 3x3 area: `(dz + 1) * 3 + (dx + 1)`.
#[inline]
pub fn slot_of(dx: i32, dz: i32) -> usize {
    debug_assert!((-1..=1).contains(&dx) && (-1..=1).contains(&dz));
    ((dz + 1) * 3 + (dx + 1)) as usize
}

/// Chunk offsets of the 3x3 area in slot order.
pub const SLOT_OFFSETS: [(i32, i32); 9] = [
    (-1, -1),
    (0, -1),
    (1, -1),
    (-1, 0),
    (0, 0),
    (1, 0),
    (-1, 1),
    (0, 1),
    (1, 1),
];

/// Writes of one decoration run, grouped by target chunk slot.
#[derive(Clone, Debug, Default)]
pub struct DecorationWrites {
    per_slot: [Vec<BlockWrite>; 9],
    entities: [Vec<EntityWrite>; 9],
}

/// Sparse metadata edits, interleaved after the indicated number of block writes.
#[derive(Clone, Debug)]
struct EntityWrite {
    after_blocks: usize,
    position: [i32; 3],
    entity: Option<BlockEntity>,
}

impl DecorationWrites {
    /// Writes into the chunk at `(dx, dz)` relative to the origin, in write order.
    pub fn for_offset(&self, dx: i32, dz: i32) -> &[BlockWrite] {
        &self.per_slot[slot_of(dx, dz)]
    }

    /// Replay both sparse actors and block writes at their original chronological points.
    pub fn apply_to(&self, dx: i32, dz: i32, chunk: &mut Chunk) {
        let slot = slot_of(dx, dz);
        let blocks = &self.per_slot[slot];
        let mut applied = 0;
        for write in &self.entities[slot] {
            for block in &blocks[applied..write.after_blocks] {
                chunk.set_block(block.x, block.y, block.z, block.block);
            }
            applied = write.after_blocks;
            match &write.entity {
                Some(entity) => {
                    chunk.set_block_entity(entity.clone());
                }
                None => {
                    let [x, y, z] = write.position;
                    chunk.remove_block_entity(x, y, z);
                }
            }
        }
        for block in &blocks[applied..] {
            chunk.set_block(block.x, block.y, block.z, block.block);
        }
    }

    /// Total number of writes.
    pub fn len(&self) -> usize {
        self.per_slot.iter().map(Vec::len).sum::<usize>()
            + self.entities.iter().map(Vec::len).sum::<usize>()
    }

    /// Whether nothing was written.
    pub fn is_empty(&self) -> bool {
        self.per_slot.iter().all(Vec::is_empty) && self.entities.iter().all(Vec::is_empty)
    }
}

type ColumnHeights = Box<[i16; 256]>;

/// [`WorldGenRegion`] over 9 cached proto chunks with copy-on-write overlays.
pub struct DecorationRegion<'a> {
    seed: i64,
    origin: ChunkPos,
    protos: [&'a ProtoChunk; 9],
    /// Copy of a proto chunk, made on the first write into it.
    overlays: [Option<Box<Chunk>>; 9],
    heightmaps: [[OnceCell<ColumnHeights>; HeightmapKind::ALL.len()]; 9],
    classes: &'static [u8],
    writes: DecorationWrites,
}

impl<'a> DecorationRegion<'a> {
    /// Region around `origin`; `protos` are in [`SLOT_OFFSETS`] order and `classes`
    /// is the generator's block-class table indexed by runtime id.
    pub fn new(
        seed: i64,
        origin: ChunkPos,
        protos: [&'a ProtoChunk; 9],
        classes: &'static [u8],
    ) -> Self {
        for (slot, proto) in protos.iter().enumerate() {
            let (dx, dz) = SLOT_OFFSETS[slot];
            assert_eq!(
                (proto.pos.x, proto.pos.z),
                (origin.x + dx, origin.z + dz),
                "decoration region proto in the wrong slot"
            );
        }
        Self {
            seed,
            origin,
            protos,
            overlays: Default::default(),
            heightmaps: Default::default(),
            classes,
            writes: DecorationWrites::default(),
        }
    }

    /// Recorded writes.
    pub fn into_writes(self) -> DecorationWrites {
        self.writes
    }

    /// Explicit removal participates in the same sparse chronological replay as insertion.
    pub fn remove_block_entity(&mut self, x: i32, y: i32, z: i32) -> bool {
        let Some((slot, _, _)) = self.locate(x, z) else {
            return false;
        };
        if !(MIN_Y..MIN_Y + WORLD_HEIGHT).contains(&y) {
            return false;
        }
        let proto = self.protos[slot];
        let chunk = self.overlays[slot].get_or_insert_with(|| Box::new(proto.chunk.clone()));
        let removed = chunk.remove_block_entity(x, y, z);
        // Record even if the origin's proto had no actor: a previous origin may
        // have supplied one by the time these cached edits are replayed.
        self.writes.entities[slot].push(EntityWrite {
            after_blocks: self.writes.per_slot[slot].len(),
            position: [x, y, z],
            entity: None,
        });
        removed
    }

    /// Slot and local coordinates of an absolute block column, if inside the 3x3 area.
    #[inline]
    fn locate(&self, x: i32, z: i32) -> Option<(usize, u8, u8)> {
        let dx = (x >> 4) - self.origin.x;
        let dz = (z >> 4) - self.origin.z;
        if !(-1..=1).contains(&dx) || !(-1..=1).contains(&dz) {
            return None;
        }
        Some((slot_of(dx, dz), (x & 15) as u8, (z & 15) as u8))
    }

    #[inline]
    fn chunk(&self, slot: usize) -> &Chunk {
        match &self.overlays[slot] {
            Some(chunk) => chunk,
            None => &self.protos[slot].chunk,
        }
    }

    #[inline]
    fn class_of(&self, block: u32) -> u8 {
        self.classes
            .get(block as usize)
            .copied()
            .unwrap_or(block_class::UNMAPPED)
    }

    fn column_top(&self, slot: usize, kind: HeightmapKind, lx: u8, lz: u8, from_y: i32) -> i16 {
        let chunk = self.chunk(slot);
        let mut y = from_y;
        while y >= MIN_Y {
            let flags = self.class_of(chunk.get_block(lx, y as i16, lz));
            assert!(
                flags & block_class::UNMAPPED == 0,
                "block {} has no class in this generator's block-class table",
                chunk.get_block(lx, y as i16, lz)
            );
            if kind.counts(flags) {
                return (y + 1) as i16;
            }
            y -= 1;
        }
        MIN_Y as i16
    }

    fn heights(&self, slot: usize, kind: HeightmapKind) -> &ColumnHeights {
        self.heightmaps[slot][kind.index()].get_or_init(|| {
            let mut out = Box::new([MIN_Y as i16; 256]);
            let top = MIN_Y + WORLD_HEIGHT - 1;
            for lz in 0..16u8 {
                for lx in 0..16u8 {
                    out[(lz as usize) << 4 | lx as usize] =
                        self.column_top(slot, kind, lx, lz, top);
                }
            }
            out
        })
    }
}

impl WorldGenRegion for DecorationRegion<'_> {
    fn seed(&self) -> i64 {
        self.seed
    }

    fn origin(&self) -> ChunkPos {
        self.origin
    }

    fn get_block(&self, x: i32, y: i32, z: i32) -> u32 {
        let Some((slot, lx, lz)) = self.locate(x, z) else {
            panic!(
                "decoration of chunk {:?} read block ({x}, {y}, {z}) outside its 3x3 region",
                self.origin
            );
        };
        if !(MIN_Y..MIN_Y + WORLD_HEIGHT).contains(&y) {
            return *blocks::AIR;
        }
        self.chunk(slot).get_block(lx, y as i16, lz)
    }

    fn set_block(&mut self, x: i32, y: i32, z: i32, block: u32) -> bool {
        let Some((slot, lx, lz)) = self.locate(x, z) else {
            return false;
        };
        if !(MIN_Y..MIN_Y + WORLD_HEIGHT).contains(&y) {
            return false;
        }
        let proto = self.protos[slot];
        let chunk = self.overlays[slot].get_or_insert_with(|| Box::new(proto.chunk.clone()));
        chunk.set_block(lx, y as i16, lz, block);
        self.writes.per_slot[slot].push(BlockWrite {
            x: lx,
            z: lz,
            y: y as i16,
            block,
        });

        // Keep initialised heightmaps current, the way Java updates them on every set.
        let flags = self.class_of(block);
        let column = (lz as usize) << 4 | lx as usize;
        for kind in HeightmapKind::ALL {
            let Some(current) = self.heightmaps[slot][kind.index()].get().map(|h| h[column]) else {
                continue;
            };
            let current = current as i32;
            let updated = if kind.counts(flags) {
                (y + 1).max(current)
            } else if y + 1 == current {
                self.column_top(slot, kind, lx, lz, y - 1) as i32
            } else {
                current
            };
            if updated != current
                && let Some(heights) = self.heightmaps[slot][kind.index()].get_mut()
            {
                heights[column] = updated as i16;
            }
        }
        true
    }

    fn set_block_entity(&mut self, entity: BlockEntity) -> bool {
        let [x, y, z] = entity.position();
        let Some((slot, _, _)) = self.locate(x, z) else {
            return false;
        };
        if !(MIN_Y..MIN_Y + WORLD_HEIGHT).contains(&y) {
            return false;
        }
        let proto = self.protos[slot];
        let chunk = self.overlays[slot].get_or_insert_with(|| Box::new(proto.chunk.clone()));
        if !chunk.set_block_entity(entity.clone()) {
            return false;
        }
        self.writes.entities[slot].push(EntityWrite {
            after_blocks: self.writes.per_slot[slot].len(),
            position: [x, y, z],
            entity: Some(entity),
        });
        true
    }

    fn heightmap(&self, kind: HeightmapKind, x: i32, z: i32) -> i32 {
        let Some((slot, lx, lz)) = self.locate(x, z) else {
            panic!(
                "decoration of chunk {:?} read the {kind:?} heightmap at ({x}, {z}) outside its 3x3 region",
                self.origin
            );
        };
        self.heights(slot, kind)[(lz as usize) << 4 | lx as usize] as i32
    }

    fn biome_at(&self, quart_x: i32, quart_y: i32, quart_z: i32) -> NativeBiome {
        let Some((slot, lx, lz)) = self.locate(quart_x << 2, quart_z << 2) else {
            panic!(
                "decoration of chunk {:?} read the biome at quart ({quart_x}, {quart_y}, {quart_z}) outside its 3x3 region",
                self.origin
            );
        };
        let qy = (quart_y - (MIN_Y >> 2)).clamp(0, QUARTS_Y as i32 - 1) as usize;
        self.protos[slot]
            .biomes
            .get(lx as usize >> 2, qy, lz as usize >> 2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::BlockRegistry;
    use std::sync::LazyLock;
    use zuri_nbt::{NBTTag, encoding::LittleEndian};

    static CLASSES: LazyLock<Vec<u8>> = LazyLock::new(|| {
        let mut classes = vec![block_class::BLOCKS_MOTION; 65536];
        classes[*blocks::AIR as usize] = block_class::AIR;
        classes
    });

    fn actor(position: [i32; 3], marker: u8) -> BlockEntity {
        BlockEntity::generated(position, "Chest", &[1, 1, b'M', marker], None, None)
    }

    fn semantics(chunk: &Chunk) -> Vec<([i32; 3], NBTTag)> {
        let mut actors: Vec<_> = chunk
            .block_entities()
            .iter()
            .map(|entity| {
                let mut bytes = Vec::new();
                entity.write_disk_nbt(&mut bytes).unwrap();
                (
                    entity.position(),
                    NBTTag::read(bytes.as_slice(), LittleEndian).unwrap(),
                )
            })
            .collect();
        actors.sort_by_key(|(position, _)| *position);
        actors
    }

    #[test]
    fn live_overlay_and_cached_replay_preserve_actor_interleaving() {
        let origin = ChunkPos::new(-1, -1);
        let chest = BlockRegistry::vanilla()
            .get_by_name("minecraft:chest")
            .unwrap();
        let mut protos = SLOT_OFFSETS
            .map(|(dx, dz)| ProtoChunk::empty(ChunkPos::new(origin.x + dx, origin.z + dz), 1));
        let center = slot_of(0, 0);
        let position = [-1, -1, -1];
        protos[center]
            .chunk
            .set_block(15, -1, 15, chest.min_state_id);
        protos[center].chunk.set_block_entity(actor(position, 1));
        let refs = std::array::from_fn(|slot| &protos[slot]);
        let mut region = DecorationRegion::new(7, origin, refs, &CLASSES);
        assert_eq!(
            semantics(region.chunk(center)),
            semantics(&protos[center].chunk)
        );
        // Metadata-only write at block offset zero, followed by a state-only update.
        assert!(region.set_block_entity(actor(position, 2)));
        region.set_block(-1, -1, -1, chest.max_state_id);
        assert_eq!(region.chunk(center).block_entities().len(), 1);
        let NBTTag::Compound(fields) = &semantics(region.chunk(center))[0].1 else {
            panic!()
        };
        assert_eq!(fields.0.get("M"), Some(&NBTTag::Byte(2.into())));
        // A different native block clears the actor, including during replay.
        region.set_block(-1, -1, -1, *blocks::STONE);
        assert!(region.chunk(center).block_entities().is_empty());
        region.set_block(-1, -1, -1, chest.min_state_id);
        region.set_block_entity(actor(position, 3));
        region.set_block_entity(actor(position, 4));
        assert_eq!(region.chunk(center).block_entities().len(), 1);
        assert!(region.remove_block_entity(-1, -1, -1));
        assert!(region.chunk(center).block_entities().is_empty());
        region.set_block_entity(actor(position, 5));
        // Removal must also apply to actors contributed by an earlier origin,
        // even when no actor was visible to this live origin.
        assert!(!region.remove_block_entity(-2, -1, -1));
        // Negative neighbour and both height edges are retained in their own slot.
        for position in [[-17, -64, -1], [-17, 319, -1]] {
            region.set_block(position[0], position[1], position[2], chest.min_state_id);
            assert!(region.set_block_entity(actor(position, 6)));
        }
        for position in [[16, 0, -1], [-33, 0, -1], [-1, -65, -1], [-1, 320, -1]] {
            assert!(!region.set_block_entity(actor(position, 7)));
        }
        let expected = SLOT_OFFSETS.map(|(dx, dz)| semantics(region.chunk(slot_of(dx, dz))));
        let writes = region.into_writes().clone();
        for (slot, (dx, dz)) in SLOT_OFFSETS.into_iter().enumerate() {
            let mut replay = protos[slot].chunk.clone();
            if slot == center {
                replay.set_block_entity(actor([-2, -1, -1], 9));
            }
            writes.apply_to(dx, dz, &mut replay);
            assert_eq!(semantics(&replay), expected[slot]);
            // Replaying the same cached origin is deterministic, not append-only.
            writes.apply_to(dx, dz, &mut replay);
            assert_eq!(semantics(&replay), expected[slot]);
        }
    }
}
