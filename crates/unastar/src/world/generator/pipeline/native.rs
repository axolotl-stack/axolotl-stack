//! Mutable native chunk history. Unlike independent decoration replay, an origin
//! observes the writes of every previously completed neighboring origin.
//!
//! This is source-world state, not a disposable memo cache: evicting it and
//! regenerating against a later biome/feature history changes native results.
//!
//! Concurrency: every chunk has its own lock. A FEATURES origin first creates
//! the terrain of its 3x3 area one chunk at a time (terrain is a pure function
//! of the position, so workers build it in parallel), then holds all nine
//! chunk locks, taken in one global order, while it decorates. Origins whose
//! areas overlap therefore run one after the other and see each other's
//! writes, exactly as under a single session lock; origins three or more
//! chunks apart run at the same time. Every interleaving equals some serial
//! order of whole origins, which is the history a single lock also allows, and
//! a single caller reproduces its own request order exactly.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

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
                    for (index, kind) in
                        [HeightmapKind::WorldSurfaceWg, HeightmapKind::OceanFloorWg]
                            .into_iter()
                            .enumerate()
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
        Self {
            proto,
            worldgen_heights: heights,
            features_complete: false,
        }
    }
}

/// One chunk of the source world, created on first use.
type Cell = Arc<Mutex<Option<NativeChunk>>>;

#[derive(Default)]
pub(super) struct NativeSession {
    cells: Mutex<HashMap<ChunkPos, Cell>>,
}

fn lock(cell: &Cell) -> MutexGuard<'_, Option<NativeChunk>> {
    cell.lock().expect("native chunk poisoned")
}

impl NativeSession {
    fn cell(&self, pos: ChunkPos) -> Cell {
        self.cells
            .lock()
            .expect("native session poisoned")
            .entry(pos)
            .or_default()
            .clone()
    }

    fn existing(&self, pos: ChunkPos) -> Option<Cell> {
        self.cells
            .lock()
            .expect("native session poisoned")
            .get(&pos)
            .cloned()
    }

    /// The chunk's cell with its terrain built. Holds no other chunk lock.
    fn created(&self, generator: &dyn Generator, pos: ChunkPos) -> Cell {
        let cell = self.cell(pos);
        {
            let mut chunk = lock(&cell);
            if chunk.is_none() {
                *chunk = Some(NativeChunk::new(generator, pos));
            }
        }
        cell
    }

    fn is_complete(&self, origin: ChunkPos) -> bool {
        self.existing(origin).is_some_and(|cell| {
            lock(&cell)
                .as_ref()
                .is_some_and(|chunk| chunk.features_complete)
        })
    }

    pub(super) fn decorate(&self, generator: &dyn Generator, origin: ChunkPos) {
        if self.is_complete(origin) {
            return;
        }
        let cells = SLOT_OFFSETS
            .map(|(dx, dz)| self.created(generator, ChunkPos::new(origin.x + dx, origin.z + dz)));
        // One global lock order (by position) makes overlapping origins wait
        // for each other without deadlock.
        let mut order: [usize; 9] = std::array::from_fn(|slot| slot);
        order.sort_by_key(|&slot| {
            let (dx, dz) = SLOT_OFFSETS[slot];
            (origin.x + dx, origin.z + dz)
        });
        let mut guards: [Option<MutexGuard<'_, Option<NativeChunk>>>; 9] = Default::default();
        for slot in order {
            guards[slot] = Some(lock(&cells[slot]));
        }
        let mut chunks = guards.map(|guard| guard.expect("every slot locked"));
        let center = SLOT_OFFSETS
            .iter()
            .position(|&offset| offset == (0, 0))
            .expect("the 3x3 contains its origin");
        if chunks[center]
            .as_ref()
            .is_some_and(|chunk| chunk.features_complete)
        {
            // Another worker completed it while this one built terrain.
            return;
        }
        {
            let protos = chunks.each_mut().map(|guard| {
                let NativeChunk {
                    proto,
                    worldgen_heights,
                    ..
                } = guard.as_mut().expect("terrain created before locking");
                (proto, &**worldgen_heights)
            });
            let mut region = DecorationRegion::new_mutable_with_bounds(
                generator.seed(),
                origin,
                protos,
                generator.block_classes(),
                generator.world_bounds(),
            );
            generator.decorate(&mut region);
        }
        chunks[center]
            .as_mut()
            .expect("terrain created before locking")
            .features_complete = true;
    }

    pub(super) fn snapshot(&self, generator: &dyn Generator, pos: ChunkPos) -> Option<Chunk> {
        let cell = self.existing(pos)?;
        let guard = lock(&cell);
        let proto = &guard.as_ref()?.proto;
        let mut chunk = proto.chunk.clone();
        chunk.set_biomes_from_quarts(&proto.biomes, u32::from);
        debug_assert_eq!(chunk.block_bounds(), generator.world_bounds());
        Some(chunk)
    }

    pub(super) fn worldgen_heightmaps(&self, pos: ChunkPos) -> Option<[[i16; 256]; 2]> {
        let cell = self.existing(pos)?;
        let guard = lock(&cell);
        Some(*guard.as_ref()?.worldgen_heights)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::block_entity::BlockEntity;
    use crate::world::chunk::BlockStateSpace;
    use crate::world::generator::{ChunkPipeline, SpawnPoint};
    use crate::world::generator::region::WorldGenRegion;
    use std::sync::{Arc, LazyLock};
    use unastar_noise::features::JavaBlock;

    static FULL_ACTORS: LazyLock<serde_json::Value> = LazyLock::new(|| {
        serde_json::from_str(include_str!(
            "../../../../tests/fixtures/native_pending_full_actor_save_boundaries_1_21_11.json"
        ))
        .unwrap()
    });

    /// Recreate the native probe's controlled input blocks and pending markers,
    /// never its expected constructor/save payloads.
    struct PendingActorGrid;

    impl Generator for PendingActorGrid {
        fn seed(&self) -> i64 {
            0
        }
        fn generate_proto(&self, pos: ChunkPos) -> ProtoChunk {
            let mut proto = ProtoChunk::empty_in_space(pos, 0, BlockStateSpace::Java);
            if pos == ChunkPos::new(0, 0) {
                for row in FULL_ACTORS["records"].as_array().unwrap() {
                    let block = JavaBlock::by_name(row["block"].as_str().unwrap()).unwrap();
                    let position = std::array::from_fn(|index| {
                        row["position"][index].as_i64().unwrap() as i32
                    });
                    let [x, y, z] = position;
                    proto.chunk.set_block(
                        x as u8,
                        y as i16,
                        z as u8,
                        u32::from(block.default_state().0),
                    );
                    assert!(
                        proto
                            .chunk
                            .set_native_block_entity(BlockEntity::pending_java(
                                position,
                                block.name()
                            ))
                    );
                }
            }
            proto
        }
        fn decorates(&self) -> bool {
            true
        }
        fn decorates_in_order(&self) -> bool {
            true
        }
        fn decorate(&self, _: &mut DecorationRegion<'_>) {}
        fn block_classes(&self) -> &'static [u8] {
            &unastar_noise::java_blocks::JAVA_STATE_CLASSES
        }
        fn bedrock_biome(&self, biome: super::super::NativeBiome) -> u32 {
            u32::from(biome)
        }
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
            let position: [i32; 3] =
                std::array::from_fn(|index| row["position"][index].as_i64().unwrap() as i32);
            let actor = proto
                .block_entities()
                .iter()
                .find(|actor| actor.position() == position)
                .unwrap();
            assert_eq!(actor.canonical_java_nbt().unwrap(), row["proto_saved"]);
        }
        let full = pipeline.generate_native(pos);
        for row in FULL_ACTORS["records"].as_array().unwrap() {
            let position: [i32; 3] =
                std::array::from_fn(|index| row["position"][index].as_i64().unwrap() as i32);
            let actor = full
                .block_entities()
                .iter()
                .find(|actor| actor.position() == position);
            if row["full_saved"].is_null() {
                assert!(actor.is_none(), "native FULL removed {}", row["block"]);
            } else {
                assert_eq!(
                    actor.unwrap().canonical_java_level_nbt().unwrap(),
                    row["full_saved"],
                    "{}",
                    row["block"]
                );
            }
        }
    }

    /// Each origin within one chunk of (0, 0) stacks one block on the shared
    /// world column (0, 0) on top of whatever earlier origins left there, and
    /// records which origins are decorating at the same time.
    struct StackingGenerator {
        decorating: std::sync::Mutex<Vec<ChunkPos>>,
        decorated: std::sync::Mutex<Vec<ChunkPos>>,
        overlapped: std::sync::atomic::AtomicBool,
    }

    fn java_state(name: &str) -> u32 {
        u32::from(JavaBlock::by_name(name).unwrap().default_state().0)
    }

    impl Generator for StackingGenerator {
        fn seed(&self) -> i64 {
            0
        }
        fn generate_proto(&self, pos: ChunkPos) -> ProtoChunk {
            let mut proto = ProtoChunk::empty_in_space(pos, 0, BlockStateSpace::Java);
            proto.chunk.fill_floor(1, java_state("minecraft:stone"));
            proto
        }
        fn decorates(&self) -> bool {
            true
        }
        fn decorates_in_order(&self) -> bool {
            true
        }
        fn decorate(&self, region: &mut DecorationRegion<'_>) {
            let origin = region.origin();
            {
                let mut decorating = self.decorating.lock().unwrap();
                // Overlapping 3x3 areas: origins at most two chunks apart.
                if decorating
                    .iter()
                    .any(|o| (o.x - origin.x).abs() <= 2 && (o.z - origin.z).abs() <= 2)
                {
                    self.overlapped
                        .store(true, std::sync::atomic::Ordering::SeqCst);
                }
                decorating.push(origin);
            }
            if origin.x.abs() <= 1 && origin.z.abs() <= 1 {
                let top = region.heightmap(HeightmapKind::WorldSurface, 0, 0);
                // Widen the window in which a concurrent writer would race.
                std::thread::sleep(std::time::Duration::from_millis(2));
                region.set_block(0, top, 0, java_state("minecraft:dirt"));
            }
            self.decorating.lock().unwrap().retain(|o| *o != origin);
            self.decorated.lock().unwrap().push(origin);
        }
        fn block_classes(&self) -> &'static [u8] {
            &unastar_noise::java_blocks::JAVA_STATE_CLASSES
        }
        fn bedrock_biome(&self, biome: super::super::NativeBiome) -> u32 {
            u32::from(biome)
        }
        fn find_spawn(&self, _: &dyn Fn(ChunkPos) -> Chunk) -> SpawnPoint {
            SpawnPoint { x: 0, y: 1, z: 0 }
        }
    }

    /// Workers decorating at the same time must still give every origin
    /// exclusive use of its 3x3 area and complete it exactly once: the nine
    /// origins around (0, 0) build one contiguous stack, none lost to a race.
    #[test]
    fn concurrent_native_origins_are_exclusive_and_complete_once() {
        let generator = Arc::new(StackingGenerator {
            decorating: Default::default(),
            decorated: Default::default(),
            overlapped: Default::default(),
        });
        let pipeline = Arc::new(ChunkPipeline::new(generator.clone()));
        let targets: Vec<ChunkPos> = (-3..=3)
            .flat_map(|x| (-3..=3).map(move |z| ChunkPos::new(x, z)))
            .collect();
        let workers: Vec<_> = (0..8)
            .map(|worker| {
                let pipeline = pipeline.clone();
                let targets: Vec<ChunkPos> =
                    targets.iter().copied().skip(worker).step_by(8).collect();
                std::thread::Builder::new()
                    .stack_size(16 << 20)
                    .spawn(move || {
                        for pos in targets {
                            pipeline.generate_native(pos);
                        }
                    })
                    .unwrap()
            })
            .collect();
        for worker in workers {
            worker.join().unwrap();
        }
        assert!(
            !generator
                .overlapped
                .load(std::sync::atomic::Ordering::SeqCst),
            "two origins with overlapping areas decorated at the same time"
        );
        let mut decorated = generator.decorated.lock().unwrap().clone();
        let total = decorated.len();
        decorated.sort_by_key(|pos| (pos.x, pos.z));
        decorated.dedup();
        assert_eq!(total, decorated.len(), "an origin was decorated twice");
        assert_eq!(decorated.len(), 9 * 9, "every origin of the 7x7 halo");
        let center = pipeline.native_snapshot(ChunkPos::new(0, 0)).unwrap();
        let dirt = java_state("minecraft:dirt");
        let stack: Vec<u32> = (1..=10).map(|y| center.get_block(0, y, 0)).collect();
        assert_eq!(&stack[..9], &[dirt; 9], "nine origins, nine stacked blocks");
        assert_ne!(stack[9], dirt);
    }
}
