//! The 3x3 decoration region.
//!
//! Decoration (features and structure pieces) of an origin chunk runs on a
//! [`DecorationRegion`]: it reads the proto chunks of the origin and its 8 neighbours plus
//! its own earlier writes, and writes anywhere in that 3x3 area. Writes are recorded per
//! target chunk; the pipeline applies, to each chunk, the writes of the 9 origins around it
//! in a fixed order. Decoration still does not see a neighbour origin's decoration;
//! this is a known Java deviation, not native order-independent equivalence.
//! Java's neighbour writes and per-thread biome-search history can make generation
//! order observable; matching only `(seed, position)` does not prove exact parity.

use std::cell::OnceCell;
use std::ops::Range;

use unastar_noise::block_class;

use super::proto::{NativeBiome, ProtoChunk, WORLD_HEIGHT};
use crate::world::ChunkPos;
use crate::world::block_entity::BlockEntity;
use crate::world::chunk::{BlockStateSpace, Chunk, MIN_Y, SUBCHUNK_COUNT, SubChunk};

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
    /// Source-native state id; native air outside height bounds. Panics outside the 3x3 area.
    fn get_block(&self, x: i32, y: i32, z: i32) -> u32;
    /// Set a block; returns `false` (and writes nothing) outside the 3x3 area or height range.
    fn set_block(&mut self, x: i32, y: i32, z: i32, block: u32) -> bool;
    /// Read primary (0) or secondary (1) storage, retaining the region's identity domain.
    fn get_block_layer(&self, x: i32, y: i32, z: i32, layer: u8) -> u32;
    /// Atomically write both Bedrock layers. Java uses a canonical primary state
    /// and never supplies a separate secondary state before conversion.
    fn set_block_layers(&mut self, x: i32, y: i32, z: i32, primary: u32, secondary: Option<u32>) -> bool;
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
    /// State id in the decoration run's explicit identity domain.
    pub block: u32,
    /// Underlying state; this write's source-domain air means no secondary layer.
    pub secondary: u32,
}

impl BlockWrite {
    fn apply_to(&self, chunk: &mut Chunk) {
        if chunk.state_space().is_air(self.secondary) {
            chunk.set_block(self.x, self.y, self.z, self.block);
        } else {
            chunk.set_block_layers(self.x, self.y, self.z, self.block, Some(self.secondary));
        }
    }
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
    state_space: BlockStateSpace,
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
        assert_eq!(chunk.state_space(), self.state_space,
            "decoration writes cannot cross Java/Bedrock state domains");
        let slot = slot_of(dx, dz);
        let blocks = &self.per_slot[slot];
        let mut applied = 0;
        for write in &self.entities[slot] {
            for block in &blocks[applied..write.after_blocks] {
                block.apply_to(chunk);
            }
            applied = write.after_blocks;
            match &write.entity {
                Some(entity) => {
                    if self.state_space == BlockStateSpace::Java {
                        chunk.set_native_block_entity(entity.clone());
                    } else {
                        chunk.set_block_entity(entity.clone());
                    }
                }
                None => {
                    let [x, y, z] = write.position;
                    chunk.remove_block_entity(x, y, z);
                }
            }
        }
        for block in &blocks[applied..] {
            block.apply_to(chunk);
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

enum RegionProtos<'a> {
    Shared([&'a ProtoChunk; 9]),
    Mutable([(&'a mut ProtoChunk, &'a [[i16; 256]; 2]); 9]),
}

impl RegionProtos<'_> {
    fn get(&self, slot: usize) -> &ProtoChunk {
        match self {
            Self::Shared(protos) => protos[slot],
            Self::Mutable(protos) => protos[slot].0,
        }
    }
}

/// [`WorldGenRegion`] over 9 proto chunks. Shared (cached) protos are read
/// through copy-on-write overlays; native session protos are mutated in place.
pub struct DecorationRegion<'a> {
    seed: i64,
    origin: ChunkPos,
    protos: RegionProtos<'a>,
    /// Shared protos only: clone just the sections this origin writes, never
    /// whole chunks or their biomes.
    overlays: [[Option<SubChunk>; SUBCHUNK_COUNT as usize]; 9],
    /// Shared protos only: sparse actors are copied when this origin changes one.
    actor_overlays: [Option<Vec<BlockEntity>>; 9],
    heightmaps: [[OnceCell<ColumnHeights>; HeightmapKind::ALL.len()]; 9],
    classes: &'static [u8],
    writes: DecorationWrites,
    bounds: Range<i32>,
}

impl<'a> DecorationRegion<'a> {
    /// Region around `origin`; `protos` are in [`SLOT_OFFSETS`] order and `classes`
    /// is the generator's block-class table indexed by source-native state id.
    pub fn new(
        seed: i64,
        origin: ChunkPos,
        protos: [&'a ProtoChunk; 9],
        classes: &'static [u8],
    ) -> Self {
        Self::new_with_bounds(seed, origin, protos, classes, MIN_Y..MIN_Y + WORLD_HEIGHT)
    }

    /// Region with the generator's actual legal build/storage blocks. The backing
    /// chunks may retain the larger shared physical storage without exposing it.
    pub fn new_with_bounds(
        seed: i64,
        origin: ChunkPos,
        protos: [&'a ProtoChunk; 9],
        classes: &'static [u8],
        bounds: Range<i32>,
    ) -> Self {
        Self::with_protos(seed, origin, RegionProtos::Shared(protos), classes, bounds)
    }

    /// A native world session owns these chunks across origin decorations. Writes
    /// mutate them directly, so later origins observe earlier neighboring writes.
    pub(crate) fn new_mutable_with_bounds(
        seed: i64,
        origin: ChunkPos,
        protos: [(&'a mut ProtoChunk, &'a [[i16; 256]; 2]); 9],
        classes: &'static [u8],
        bounds: Range<i32>,
    ) -> Self {
        Self::with_protos(seed, origin, RegionProtos::Mutable(protos), classes, bounds)
    }

    fn with_protos(
        seed: i64,
        origin: ChunkPos,
        protos: RegionProtos<'a>,
        classes: &'static [u8],
        bounds: Range<i32>,
    ) -> Self {
        assert!(
            bounds.start >= MIN_Y && bounds.end <= MIN_Y + WORLD_HEIGHT
                && bounds.start < bounds.end
                && bounds.start % 16 == 0 && bounds.end % 16 == 0,
            "decoration bounds {bounds:?} exceed physical chunk storage"
        );
        for (slot, &(dx, dz)) in SLOT_OFFSETS.iter().enumerate() {
            let proto = protos.get(slot);
            assert_eq!(
                (proto.pos.x, proto.pos.z),
                (origin.x + dx, origin.z + dz),
                "decoration region proto in the wrong slot"
            );
            assert_eq!(proto.chunk.state_space(), protos.get(0).chunk.state_space(),
                "decoration region mixes Java and Bedrock proto identities");
        }
        let state_space = protos.get(0).chunk.state_space();
        Self {
            seed,
            origin,
            protos,
            bounds,
            overlays: Default::default(),
            actor_overlays: Default::default(),
            heightmaps: Default::default(),
            classes,
            writes: DecorationWrites {
                state_space,
                ..Default::default()
            },
        }
    }

    /// Identity domain of all protos, overlays and recorded writes in this region.
    pub fn state_space(&self) -> BlockStateSpace {
        self.protos.get(0).chunk.state_space()
    }

    /// Recorded writes.
    pub fn into_writes(self) -> DecorationWrites {
        self.writes
    }

    /// Saved actor visible in the current native/overlay chunk. Pending native
    /// metadata can exist outside build height; do not erase it on inspection.
    pub fn block_entity(&self, x: i32, y: i32, z: i32) -> Option<&BlockEntity> {
        let (slot, _, _) = self.locate(x, z)?;
        if !self.bounds.contains(&y) && self.state_space() != BlockStateSpace::Java {
            return None;
        }
        self.actors(slot).iter().find(|entity| entity.position() == [x, y, z])
    }

    /// Native WorldGenRegion can install pending NBT after a height-rejected block
    /// write. Ordinary region/storage insertion remains build-height checked.
    pub fn set_native_block_entity(&mut self, entity: BlockEntity) -> bool {
        assert_eq!(self.state_space(), BlockStateSpace::Java);
        self.insert_block_entity(entity, true)
    }

    fn insert_block_entity(&mut self, entity: BlockEntity, native: bool) -> bool {
        let [x, y, z] = entity.position();
        let Some((slot, _, _)) = self.locate(x, z) else {
            return false;
        };
        if !native && !self.bounds.contains(&y) {
            return false;
        }
        let record = matches!(self.protos, RegionProtos::Shared(_));
        let recorded = record.then(|| entity.clone());
        let inserted = match &mut self.protos {
            RegionProtos::Mutable(protos) => {
                let chunk = &mut protos[slot].0.chunk;
                if native { chunk.set_native_block_entity(entity) } else { chunk.set_block_entity(entity) }
            }
            RegionProtos::Shared(protos) => {
                // Mirror Chunk::{set_block_entity, set_native_block_entity}: the
                // slot already fixes the chunk; only ordinary actors are height checked.
                if native || protos[slot].chunk.block_bounds().contains(&y) {
                    let actors = self.actor_overlays[slot]
                        .get_or_insert_with(|| protos[slot].chunk.block_entities().to_vec());
                    match actors.iter_mut().find(|old| old.position() == [x, y, z]) {
                        Some(old) => *old = entity,
                        None => actors.push(entity),
                    }
                    true
                } else {
                    false
                }
            }
        };
        if inserted && let Some(entity) = recorded {
            self.writes.entities[slot].push(EntityWrite {
                after_blocks: self.writes.per_slot[slot].len(),
                position: [x, y, z],
                entity: Some(entity),
            });
        }
        inserted
    }

    /// Explicit removal participates in the same sparse chronological replay as insertion.
    pub fn remove_block_entity(&mut self, x: i32, y: i32, z: i32) -> bool {
        let Some((slot, _, _)) = self.locate(x, z) else {
            return false;
        };
        if !self.bounds.contains(&y) {
            return false;
        }
        let removed = match &mut self.protos {
            RegionProtos::Mutable(protos) => protos[slot].0.chunk.remove_block_entity(x, y, z),
            RegionProtos::Shared(protos) => {
                let chunk = &protos[slot].chunk;
                match chunk.block_bounds().contains(&y).then(|| {
                    self.actor_overlays[slot].as_deref().unwrap_or(chunk.block_entities())
                        .iter().position(|actor| actor.position() == [x, y, z])
                }).flatten() {
                    Some(index) => {
                        self.actor_overlays[slot]
                            .get_or_insert_with(|| chunk.block_entities().to_vec())
                            .remove(index);
                        true
                    }
                    None => false,
                }
            }
        };
        // Independent replay must record removals even if this proto had no actor.
        if matches!(self.protos, RegionProtos::Shared(_)) {
            self.writes.entities[slot].push(EntityWrite {
                after_blocks: self.writes.per_slot[slot].len(),
                position: [x, y, z],
                entity: None,
            });
        }
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

    /// Current block in one storage layer: the session chunk, or a shared proto
    /// seen through this origin's section overlay.
    #[inline]
    fn block_layer(&self, slot: usize, x: u8, y: i16, z: u8, layer: u8) -> u32 {
        let chunk = &self.protos.get(slot).chunk;
        if matches!(self.protos, RegionProtos::Shared(_))
            && chunk.block_bounds().contains(&i32::from(y))
            && let Some(section) = &self.overlays[slot][((i32::from(y) - MIN_Y) >> 4) as usize]
        {
            let local_y = (y & 15) as u8;
            return if layer == 0 {
                section.get_block(x, local_y, z)
            } else {
                section.secondary_block(x, local_y, z).unwrap_or_else(|| chunk.state_space().air())
            };
        }
        chunk.get_block_layer(x, y, z, layer)
    }

    fn actors(&self, slot: usize) -> &[BlockEntity] {
        self.actor_overlays[slot]
            .as_deref()
            .unwrap_or_else(|| self.protos.get(slot).chunk.block_entities())
    }

    /// Apply one block write with exactly the chunk's storage/actor semantics.
    fn write_block(&mut self, slot: usize, x: u8, y: i16, z: u8, block: u32, secondary: Option<u32>) {
        let space = self.state_space();
        let protos = match &mut self.protos {
            RegionProtos::Mutable(protos) => {
                let chunk = &mut protos[slot].0.chunk;
                if space == BlockStateSpace::Java {
                    chunk.set_block(x, y, z, block);
                } else {
                    chunk.set_block_layers(x, y, z, block, secondary);
                }
                return;
            }
            RegionProtos::Shared(protos) => protos,
        };
        let chunk = &protos[slot].chunk;
        if !chunk.block_bounds().contains(&i32::from(y)) {
            return;
        }
        let section_index = ((i32::from(y) - MIN_Y) >> 4) as usize;
        let old = self.overlays[slot][section_index]
            .get_or_insert_with(|| chunk.sub_chunk(section_index).clone())
            .set_block_layers(x, (y & 15) as u8, z, block, secondary);
        if old != block && !space.same_block(old, block) {
            let position = [(chunk.x << 4) | i32::from(x), i32::from(y), (chunk.z << 4) | i32::from(z)];
            let actors = self.actor_overlays[slot].as_deref().unwrap_or(chunk.block_entities());
            if let Some(index) = actors.iter().position(|actor| actor.position() == position) {
                self.actor_overlays[slot]
                    .get_or_insert_with(|| chunk.block_entities().to_vec())
                    .remove(index);
            }
        }
    }

    #[inline]
    fn class_of(&self, block: u32) -> u8 {
        self.classes
            .get(block as usize)
            .copied()
            .unwrap_or(block_class::UNMAPPED)
    }

    /// Combined layer flags; `frozen` reads the unmodified proto (Java WG maps).
    fn cell_flags(&self, slot: usize, frozen: bool, x: u8, y: i16, z: u8) -> u8 {
        let (primary, secondary) = if frozen {
            let chunk = &self.protos.get(slot).chunk;
            (chunk.get_block(x, y, z), chunk.get_block_layer(x, y, z, 1))
        } else {
            (self.block_layer(slot, x, y, z, 0), self.block_layer(slot, x, y, z, 1))
        };
        let mut flags = self.class_of(primary);
        if !self.state_space().is_air(secondary) {
            flags = (flags | self.class_of(secondary)) & !block_class::AIR;
        }
        flags
    }

    fn column_top(&self, slot: usize, kind: HeightmapKind, lx: u8, lz: u8, from_y: i32) -> i16 {
        let frozen = self.state_space() == BlockStateSpace::Java
            && matches!(kind, HeightmapKind::WorldSurfaceWg | HeightmapKind::OceanFloorWg);
        let mut y = from_y.min(self.bounds.end - 1);
        while y >= self.bounds.start {
            let flags = self.cell_flags(slot, frozen, lx, y as i16, lz);
            assert!(
                flags & block_class::UNMAPPED == 0,
                "block {} has no class in this generator's block-class table",
                self.block_layer(slot, lx, y as i16, lz, 0)
            );
            if kind.counts(flags) {
                return (y + 1) as i16;
            }
            y -= 1;
        }
        self.bounds.start as i16
    }

    fn heights(&self, slot: usize, kind: HeightmapKind) -> &[i16; 256] {
        if matches!(kind, HeightmapKind::WorldSurfaceWg | HeightmapKind::OceanFloorWg)
            && let RegionProtos::Mutable(protos) = &self.protos
        {
            return &protos[slot].1[kind.index()];
        }
        self.heightmaps[slot][kind.index()].get_or_init(|| {
            let mut out = Box::new([self.bounds.start as i16; 256]);
            let top = self.bounds.end - 1;
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

    fn min_y(&self) -> i32 {
        self.bounds.start
    }

    fn height(&self) -> i32 {
        self.bounds.end - self.bounds.start
    }

    fn get_block(&self, x: i32, y: i32, z: i32) -> u32 {
        self.get_block_layer(x, y, z, 0)
    }

    fn get_block_layer(&self, x: i32, y: i32, z: i32, layer: u8) -> u32 {
        let Some((slot, lx, lz)) = self.locate(x, z) else {
            panic!(
                "decoration of chunk {:?} read block ({x}, {y}, {z}) outside its 3x3 region",
                self.origin
            );
        };
        if !self.bounds.contains(&y) {
            return self.state_space().air();
        }
        self.block_layer(slot, lx, y as i16, lz, layer)
    }

    fn set_block(&mut self, x: i32, y: i32, z: i32, block: u32) -> bool {
        self.set_block_layers(x, y, z, block, None)
    }

    fn set_block_layers(
        &mut self, x: i32, y: i32, z: i32, block: u32, secondary: Option<u32>,
    ) -> bool {
        let Some((slot, lx, lz)) = self.locate(x, z) else {
            return false;
        };
        if !self.bounds.contains(&y) {
            return false;
        }
        let space = self.state_space();
        let secondary_state = secondary.unwrap_or_else(|| space.air());
        if space == BlockStateSpace::Java {
            assert!(secondary.is_none(), "Java waterlogging must remain in its canonical primary state");
        }
        self.write_block(slot, lx, y as i16, lz, block, secondary);
        if matches!(self.protos, RegionProtos::Shared(_)) {
            self.writes.per_slot[slot].push(BlockWrite {
                x: lx,
                z: lz,
                y: y as i16,
                block,
                secondary: secondary_state,
            });
        }

        // Keep initialised heightmaps current, the way Java updates them on every set.
        let flags = self.cell_flags(slot, false, lx, y as i16, lz);
        let column = (lz as usize) << 4 | lx as usize;
        for kind in HeightmapKind::ALL {
            // Persisted CARVERS selects the four final maps. The WG maps remain
            // frozen at carver completion throughout native FEATURES writes.
            if self.state_space() == BlockStateSpace::Java
                && matches!(kind, HeightmapKind::WorldSurfaceWg | HeightmapKind::OceanFloorWg)
            {
                continue;
            }
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
        self.insert_block_entity(entity, false)
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
        let quart_y = quart_y.clamp(self.bounds.start >> 2, (self.bounds.end - 1) >> 2);
        let qy = (quart_y - (MIN_Y >> 2)) as usize;
        self.protos.get(slot)
            .biomes
            .get(lx as usize >> 2, qy, lz as usize >> 2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::BlockRegistry;
    use crate::world::chunk::blocks;
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

    fn semantics(actors: &[BlockEntity]) -> Vec<([i32; 3], NBTTag)> {
        let mut actors: Vec<_> = actors
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
            semantics(region.actors(center)),
            semantics(protos[center].chunk.block_entities())
        );
        // Metadata-only write at block offset zero, followed by a state-only update.
        assert!(region.set_block_entity(actor(position, 2)));
        region.set_block(-1, -1, -1, chest.max_state_id);
        assert_eq!(region.actors(center).len(), 1);
        let NBTTag::Compound(fields) = &semantics(region.actors(center))[0].1 else {
            panic!()
        };
        assert_eq!(fields.0.get("M"), Some(&NBTTag::Byte(2.into())));
        // A different native block clears the actor, including during replay.
        region.set_block(-1, -1, -1, *blocks::STONE);
        assert!(region.actors(center).is_empty());
        region.set_block(-1, -1, -1, chest.min_state_id);
        region.set_block_entity(actor(position, 3));
        region.set_block_entity(actor(position, 4));
        assert_eq!(region.actors(center).len(), 1);
        assert!(region.remove_block_entity(-1, -1, -1));
        assert!(region.actors(center).is_empty());
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
        let expected = SLOT_OFFSETS.map(|(dx, dz)| semantics(region.actors(slot_of(dx, dz))));
        let writes = region.into_writes().clone();
        for (slot, (dx, dz)) in SLOT_OFFSETS.into_iter().enumerate() {
            let mut replay = protos[slot].chunk.clone();
            if slot == center {
                replay.set_block_entity(actor([-2, -1, -1], 9));
            }
            writes.apply_to(dx, dz, &mut replay);
            assert_eq!(semantics(replay.block_entities()), expected[slot]);
            // Replaying the same cached origin is deterministic, not append-only.
            writes.apply_to(dx, dz, &mut replay);
            assert_eq!(semantics(replay.block_entities()), expected[slot]);
        }
    }

    #[test]
    fn native_dimension_consumers_observe_storage_not_noise_bounds() {
        use crate::world::generator::JavaDimension;
        use unastar_noise::features::{java_blocks::states, eval::BlockEntityData};

        let air = u32::from(states::AIR.0);
        let stone = u32::from(states::STONE.0);
        let chest = u32::from(states::CHEST.0);
        let origin = ChunkPos::new(0, 0);
        for dimension in [JavaDimension::Overworld, JavaDimension::Nether, JavaDimension::End] {
            let bounds = dimension.block_bounds();
            let mut protos = SLOT_OFFSETS.map(|(dx, dz)| {
                ProtoChunk::empty_in_space(ChunkPos::new(dx, dz), 1, BlockStateSpace::Java)
            });
            let center = slot_of(0, 0);
            // Dirty physical padding must stay hidden from blocks and heightmaps.
            if bounds.start > MIN_Y {
                protos[center].chunk.set_block(0, -1, 0, stone);
                protos[center].chunk.set_block(0, 319, 0, stone);
            }
            let bottom_quart = ((bounds.start - MIN_Y) >> 2) as usize;
            let top_quart = ((bounds.end - 1 - MIN_Y) >> 2) as usize;
            protos[center].biomes.set(0, bottom_quart, 0, 2);
            protos[center].biomes.set(0, top_quart, 0, 3);
            let refs = std::array::from_fn(|slot| &protos[slot]);
            let mut region = DecorationRegion::new_with_bounds(
                7, origin, refs, &unastar_noise::java_blocks::JAVA_STATE_CLASSES, bounds.clone(),
            );
            assert_eq!((region.min_y(), region.height()), (bounds.start, bounds.end - bounds.start));
            assert_eq!(region.heightmap(HeightmapKind::WorldSurfaceWg, 0, 0), bounds.start);
            assert_eq!(region.get_block(0, bounds.start - 1, 0), air);
            assert_eq!(region.get_block(0, bounds.end, 0), air);
            assert_eq!(region.biome_at(0, i32::MIN, 0), 2);
            assert_eq!(region.biome_at(0, i32::MAX, 0), 3);
            for y in [bounds.start - 1, bounds.end] {
                assert!(!region.set_block(0, y, 0, chest));
                assert!(!region.set_block_entity(BlockEntity::generated_java(
                    [0, y, 0], "minecraft:chest", BlockEntityData::loot("minecraft:chests/simple_dungeon", 7),
                )));
                assert!(!region.remove_block_entity(0, y, 0));
            }
            // Legal storage is not restricted to the preset's noise-fill height.
            let top = bounds.end - 1;
            assert!(region.set_block(0, top, 0, chest));
            assert!(region.set_block_entity(BlockEntity::generated_java(
                [0, top, 0], "minecraft:chest", BlockEntityData::loot("minecraft:chests/simple_dungeon", 7),
            )));
            assert_eq!(region.get_block(0, top, 0), chest);
            assert_eq!(region.heightmap(HeightmapKind::WorldSurfaceWg, 0, 0), bounds.start);
            assert_eq!(region.heightmap(HeightmapKind::WorldSurface, 0, 0), bounds.end);
            assert!(region.set_block(0, top, 0, air));
            assert_eq!(region.heightmap(HeightmapKind::WorldSurface, 0, 0), bounds.start);
            assert!(region.set_block(0, top, 0, chest));
            assert!(region.set_block_entity(BlockEntity::generated_java(
                [0, top, 0], "minecraft:chest", BlockEntityData::loot("minecraft:chests/simple_dungeon", 9),
            )));
            let writes = region.into_writes();
            assert!(writes.for_offset(0, 0).iter().all(|write| bounds.contains(&i32::from(write.y))));
            let mut replay = protos[center].chunk.clone();
            replay.set_block_bounds(bounds.clone());
            writes.apply_to(0, 0, &mut replay);
            assert_eq!(replay.get_block(0, top as i16, 0), chest);
            assert_eq!(replay.block_entities().len(), 1);
            assert_eq!(replay.block_entities()[0].position(), [0, top, 0]);
        }
        assert_eq!(unastar_noise::nether_settings::HEIGHT, 128);
        assert_eq!(unastar_noise::end_settings::HEIGHT, 128);
        assert_eq!(JavaDimension::Nether.block_bounds(), 0..256);
        assert_eq!(JavaDimension::End.block_bounds(), 0..256);
    }
    #[test]
    fn underlying_water_participates_in_region_reads_heights_and_atomic_replay() {
        let origin = ChunkPos::new(-1, -1);
        let protos = SLOT_OFFSETS.map(|(dx, dz)| {
            ProtoChunk::empty(ChunkPos::new(origin.x + dx, origin.z + dz), 1)
        });
        let refs = std::array::from_fn(|slot| &protos[slot]);
        let mut region = DecorationRegion::new(
            7, origin, refs, &unastar_noise::java_blocks::BEDROCK_STATE_CLASSES,
        );
        assert_eq!(region.heightmap(HeightmapKind::WorldSurface, -1, -1), MIN_Y);
        assert_eq!(region.heightmap(HeightmapKind::MotionBlocking, -1, -1), MIN_Y);
        assert!(region.set_block_layers(-1, 2, -1, *blocks::AIR, Some(*blocks::WATER)));
        assert!(region.set_block_layers(-2, 2, -1, *blocks::STONE, Some(*blocks::WATER)));
        assert!(region.set_block(-2, 2, -1, *blocks::STONE));
        assert_eq!(region.get_block(-1, 2, -1), *blocks::AIR);
        assert_eq!(region.get_block_layer(-1, 2, -1, 1), *blocks::WATER);
        assert_eq!(region.get_block_layer(-2, 2, -1, 1), *blocks::AIR);
        assert_eq!(region.heightmap(HeightmapKind::WorldSurface, -1, -1), 3);
        assert_eq!(region.heightmap(HeightmapKind::MotionBlocking, -1, -1), 3);
        let writes = region.into_writes();
        let mut replay = protos[slot_of(0, 0)].chunk.clone();
        writes.apply_to(0, 0, &mut replay);
        assert_eq!(replay.get_block(15, 2, 15), *blocks::AIR);
        assert_eq!(replay.get_block_layer(15, 2, 15, 1), *blocks::WATER);
        assert_eq!(replay.get_block(14, 2, 15), *blocks::STONE);
        assert_eq!(replay.get_block_layer(14, 2, 15, 1), *blocks::AIR);
        assert_eq!(replay.encode_subchunk_disk(0).unwrap()[1], 2);
    }

    #[test]
    fn section_edges_and_heightmaps_replay_at_negative_neighbor_coordinates() {
        let origin = ChunkPos::new(-2, -3);
        let protos = SLOT_OFFSETS
            .map(|(dx, dz)| ProtoChunk::empty(ChunkPos::new(origin.x + dx, origin.z + dz), 1));
        let refs = std::array::from_fn(|slot| &protos[slot]);
        let mut region = DecorationRegion::new(7, origin, refs, &CLASSES);
        let (x, z) = (-33, -49);
        let levels = [-64, -49, -48, -1, 0, 15, 16, 319];
        assert_eq!(region.heightmap(HeightmapKind::WorldSurfaceWg, x, z), MIN_Y);
        for y in levels {
            assert!(region.set_block(x, y, z, *blocks::DIRT));
            assert_eq!(region.get_block(x, y, z), *blocks::DIRT);
            assert_eq!(region.get_block(x - 1, y, z), *blocks::AIR);
        }
        assert_eq!(region.heightmap(HeightmapKind::WorldSurfaceWg, x, z), 320);
        region.set_block(x, 319, z, *blocks::AIR);
        assert_eq!(region.heightmap(HeightmapKind::WorldSurfaceWg, x, z), 17);
        assert!(!region.set_block(x, -65, z, *blocks::STONE));
        assert!(!region.set_block(x, 320, z, *blocks::STONE));
        let writes = region.into_writes();
        let mut replay = protos[slot_of(-1, -1)].chunk.clone();
        writes.apply_to(-1, -1, &mut replay);
        for y in levels {
            let expected = if y == 319 { *blocks::AIR } else { *blocks::DIRT };
            assert_eq!(replay.get_block(15, y as i16, 15), expected);
        }
    }
}
