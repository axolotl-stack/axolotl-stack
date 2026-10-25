//! Generator trait and the chunk pipeline shared by every world generator.
//!
//! ```text
//! generate(c):
//!   proto(p)       for p in the 5x5 around c      (noise -> aquifers/veins -> 3D biomes
//!                                                   -> surface -> carvers; cached)
//!   decoration(o)  for o in the 3x3 around c      (features + structure pieces on a
//!                                                   DecorationRegion over o's 3x3 protos;
//!                                                   cached as per-target write lists)
//!   chunk(c) = proto(c).chunk + writes of the 9 origins targeting c, in SLOT_OFFSETS order
//!              + Bedrock 3D biomes from proto(c).biomes
//! ```
//!
//! Both caches are single-flight: concurrent workers asking for the same proto or
//! decoration wait for one computation instead of repeating it. Eviction only costs time;
//! every finished chunk is a pure function of `(seed, position)` (see [`super::region`]).

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, OnceLock};

use super::proto::{NativeBiome, ProtoChunk};
use super::region::{DecorationRegion, DecorationWrites, SLOT_OFFSETS};
use crate::world::ChunkPos;
use crate::world::chunk::Chunk;

/// A spawn position chosen by a generator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpawnPoint {
    /// Block x.
    pub x: i32,
    /// Block y of the block the player stands in (one above the floor).
    pub y: i32,
    /// Block z.
    pub z: i32,
}

/// A world generator: terrain stages per chunk plus an optional 3x3 decoration stage.
///
/// Implementations must be pure: the same `(seed, position)` always gives the same
/// output, independent of call order or thread.
pub trait Generator: Send + Sync + 'static {
    /// World seed.
    fn seed(&self) -> i64;

    /// Terrain stages for one chunk: noise, aquifers, ore veins, 3D biomes, surface, carvers.
    fn generate_proto(&self, pos: ChunkPos) -> ProtoChunk;

    /// Whether [`Generator::decorate`] places anything. Generators without decoration
    /// (flat, void) skip the 3x3 stage entirely.
    fn decorates(&self) -> bool;

    /// Decorate the region's origin chunk (features and structure pieces).
    fn decorate(&self, region: &mut DecorationRegion<'_>);

    /// Block-class flags (`unastar_noise::block_class`) indexed by runtime id, used for
    /// the region's heightmaps.
    fn block_classes(&self) -> &'static [u8];

    /// Bedrock biome id for a generator-native biome.
    fn bedrock_biome(&self, biome: NativeBiome) -> u32;

    /// World spawn. `chunk_at` returns finished chunks (decorated, with biomes) for
    /// generators that search the generated terrain.
    fn find_spawn(&self, chunk_at: &dyn Fn(ChunkPos) -> Chunk) -> SpawnPoint;
}

/// Bounded single-flight cache keyed by chunk position (FIFO eviction).
pub(crate) struct ChunkCache<V> {
    capacity: usize,
    inner: Mutex<CacheInner<V>>,
}

struct CacheInner<V> {
    map: HashMap<ChunkPos, Arc<OnceLock<Arc<V>>>>,
    order: VecDeque<ChunkPos>,
}

impl<V> ChunkCache<V> {
    pub(crate) fn new(capacity: usize) -> Self {
        assert!(capacity > 0, "chunk cache capacity must be positive");
        Self {
            capacity,
            inner: Mutex::new(CacheInner {
                map: HashMap::with_capacity(capacity),
                order: VecDeque::with_capacity(capacity),
            }),
        }
    }

    pub(crate) fn get_or_compute(&self, pos: ChunkPos, compute: impl FnOnce() -> V) -> Arc<V> {
        let cell = {
            let mut inner = self.inner.lock().expect("chunk cache poisoned");
            if let Some(cell) = inner.map.get(&pos) {
                cell.clone()
            } else {
                if inner.order.len() >= self.capacity
                    && let Some(oldest) = inner.order.pop_front()
                {
                    inner.map.remove(&oldest);
                }
                let cell = Arc::new(OnceLock::new());
                inner.map.insert(pos, cell.clone());
                inner.order.push_back(pos);
                cell
            }
        };
        cell.get_or_init(|| Arc::new(compute())).clone()
    }

    fn len(&self) -> usize {
        self.inner.lock().expect("chunk cache poisoned").map.len()
    }
}

/// Default number of cached protos: a 32x32 chunk area.
pub const DEFAULT_PROTO_CACHE: usize = 1024;

/// Drives a [`Generator`] through proto, decoration and finalisation with caches.
pub struct ChunkPipeline {
    generator: Arc<dyn Generator>,
    protos: ChunkCache<ProtoChunk>,
    decorations: ChunkCache<DecorationWrites>,
}

impl ChunkPipeline {
    /// Pipeline with the default cache sizes.
    pub fn new(generator: Arc<dyn Generator>) -> Self {
        Self::with_capacity(generator, DEFAULT_PROTO_CACHE)
    }

    /// Pipeline caching up to `proto_capacity` protos and as many decorations.
    pub fn with_capacity(generator: Arc<dyn Generator>, proto_capacity: usize) -> Self {
        Self {
            generator,
            protos: ChunkCache::new(proto_capacity),
            decorations: ChunkCache::new(proto_capacity),
        }
    }

    /// The generator this pipeline drives.
    pub fn generator(&self) -> &Arc<dyn Generator> {
        &self.generator
    }

    /// Cached (or freshly generated) proto chunk.
    pub fn proto(&self, pos: ChunkPos) -> Arc<ProtoChunk> {
        self.protos
            .get_or_compute(pos, || self.generator.generate_proto(pos))
    }

    /// Cached (or freshly computed) decoration writes of an origin chunk.
    pub fn decoration(&self, origin: ChunkPos) -> Arc<DecorationWrites> {
        self.decorations.get_or_compute(origin, || {
            let protos: [Arc<ProtoChunk>; 9] = SLOT_OFFSETS
                .map(|(dx, dz)| self.proto(ChunkPos::new(origin.x + dx, origin.z + dz)));
            let refs: [&ProtoChunk; 9] = std::array::from_fn(|slot| &*protos[slot]);
            let mut region = DecorationRegion::new(
                self.generator.seed(),
                origin,
                refs,
                self.generator.block_classes(),
            );
            self.generator.decorate(&mut region);
            region.into_writes()
        })
    }

    /// Finished chunk: proto blocks, decoration from the 9 surrounding origins, and
    /// Bedrock 3D biomes. Generators without decoration need no neighbours, so their protos
    /// are used directly instead of being cached and copied.
    pub fn generate(&self, pos: ChunkPos) -> Chunk {
        let generator = &self.generator;
        if !generator.decorates() {
            let ProtoChunk {
                mut chunk, biomes, ..
            } = generator.generate_proto(pos);
            chunk.set_biomes_from_quarts(&biomes, |biome| generator.bedrock_biome(biome));
            chunk.x = pos.x;
            chunk.z = pos.z;
            return chunk;
        }
        let proto = self.proto(pos);
        let mut chunk = proto.chunk.clone();
        for (dx, dz) in SLOT_OFFSETS {
            let origin = ChunkPos::new(pos.x + dx, pos.z + dz);
            let writes = self.decoration(origin);
            // `pos` seen from `origin` is at (-dx, -dz).
            writes.apply_to(-dx, -dz, &mut chunk);
        }
        chunk.set_biomes_from_quarts(&proto.biomes, |biome| generator.bedrock_biome(biome));
        chunk.x = pos.x;
        chunk.z = pos.z;
        chunk
    }

    /// World spawn chosen by the generator over this pipeline's finished chunks.
    pub fn find_spawn(&self) -> SpawnPoint {
        self.generator.find_spawn(&|pos| self.generate(pos))
    }

    /// Number of cached protos (for benchmarks and tests).
    pub fn cached_protos(&self) -> usize {
        self.protos.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::chunk::blocks;
    use crate::world::generator::region::{HeightmapKind, WorldGenRegion};
    use std::sync::LazyLock;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Air is air, everything else blocks motion.
    static TEST_CLASSES: LazyLock<Vec<u8>> = LazyLock::new(|| {
        let mut classes = vec![unastar_noise::block_class::BLOCKS_MOTION; 65536];
        classes[*blocks::AIR as usize] = unastar_noise::block_class::AIR;
        classes
    });

    /// Stone floor at y=0; decoration puts a 3-block pillar of dirt on the origin's
    /// (15, 15) corner column and a marker block just across the +x/+z border.
    struct PillarGenerator {
        protos_made: AtomicUsize,
    }

    impl PillarGenerator {
        fn new() -> Self {
            Self {
                protos_made: AtomicUsize::new(0),
            }
        }
    }

    impl Generator for PillarGenerator {
        fn seed(&self) -> i64 {
            7
        }
        fn generate_proto(&self, pos: ChunkPos) -> ProtoChunk {
            self.protos_made.fetch_add(1, Ordering::SeqCst);
            let mut proto = ProtoChunk::empty(pos, 1);
            proto.chunk.fill_floor(1, *blocks::STONE);
            proto
        }
        fn decorates(&self) -> bool {
            true
        }
        fn decorate(&self, region: &mut DecorationRegion<'_>) {
            let origin = region.origin();
            let (x, z) = (origin.x * 16 + 15, origin.z * 16 + 15);
            let top = region.heightmap(HeightmapKind::WorldSurfaceWg, x, z);
            for y in top..top + 3 {
                region.set_block(x, y, z, *blocks::DIRT);
            }
            // Spill into the +x/+z neighbour on top of the floor.
            let spill = region.heightmap(HeightmapKind::WorldSurfaceWg, x + 1, z + 1);
            region.set_block(x + 1, spill, z + 1, *blocks::GRASS_BLOCK);
        }
        fn block_classes(&self) -> &'static [u8] {
            &TEST_CLASSES
        }
        fn bedrock_biome(&self, biome: NativeBiome) -> u32 {
            biome as u32
        }
        fn find_spawn(&self, _chunk_at: &dyn Fn(ChunkPos) -> Chunk) -> SpawnPoint {
            SpawnPoint { x: 0, y: 1, z: 0 }
        }
    }

    fn snapshot(chunk: &Chunk) -> Vec<u32> {
        let mut out = Vec::new();
        for y in -1..5i16 {
            for z in 0..16u8 {
                for x in 0..16u8 {
                    out.push(chunk.get_block(x, y, z));
                }
            }
        }
        out
    }

    #[test]
    fn decoration_spills_into_neighbours_and_is_order_independent() {
        let forward = ChunkPipeline::new(Arc::new(PillarGenerator::new()));
        let a = forward.generate(ChunkPos::new(0, 0));
        let b = forward.generate(ChunkPos::new(1, 1));

        let backward = ChunkPipeline::new(Arc::new(PillarGenerator::new()));
        let b2 = backward.generate(ChunkPos::new(1, 1));
        let a2 = backward.generate(ChunkPos::new(0, 0));

        assert_eq!(snapshot(&a), snapshot(&a2));
        assert_eq!(snapshot(&b), snapshot(&b2));
        // Own pillar on top of the y=0 floor.
        assert_eq!(a.get_block(15, 1, 15), *blocks::DIRT);
        assert_eq!(a.get_block(15, 3, 15), *blocks::DIRT);
        // The (-1,-1) origin's spill lands on (0, 0) of chunk (0, 0).
        assert_eq!(a.get_block(0, 1, 0), *blocks::GRASS_BLOCK);
        // And chunk (0,0)'s spill lands on (0, 0) of chunk (1, 1).
        assert_eq!(b.get_block(0, 1, 0), *blocks::GRASS_BLOCK);
    }

    #[test]
    fn protos_are_generated_once_per_position() {
        let generator = Arc::new(PillarGenerator::new());
        let pipeline = ChunkPipeline::new(generator.clone());
        pipeline.generate(ChunkPos::new(0, 0));
        // 5x5 protos for one finished chunk.
        assert_eq!(generator.protos_made.load(Ordering::SeqCst), 25);
        pipeline.generate(ChunkPos::new(1, 0));
        // Only the new column of 5.
        assert_eq!(generator.protos_made.load(Ordering::SeqCst), 30);
    }

    #[test]
    fn heightmap_tracks_writes_in_the_region() {
        let generator = PillarGenerator::new();
        let protos: [ProtoChunk; 9] =
            SLOT_OFFSETS.map(|(dx, dz)| generator.generate_proto(ChunkPos::new(dx, dz)));
        let refs: [&ProtoChunk; 9] = std::array::from_fn(|slot| &protos[slot]);
        let mut region = DecorationRegion::new(7, ChunkPos::new(0, 0), refs, &TEST_CLASSES);
        assert_eq!(region.heightmap(HeightmapKind::WorldSurface, 3, 4), 1);
        region.set_block(3, 10, 4, *blocks::DIRT);
        assert_eq!(region.heightmap(HeightmapKind::WorldSurface, 3, 4), 11);
        region.set_block(3, 10, 4, *blocks::AIR);
        assert_eq!(region.heightmap(HeightmapKind::WorldSurface, 3, 4), 1);
        assert!(!region.set_block(40, 10, 4, *blocks::DIRT));
    }

    struct ActorGenerator;

    impl Generator for ActorGenerator {
        fn seed(&self) -> i64 {
            7
        }
        fn generate_proto(&self, pos: ChunkPos) -> ProtoChunk {
            use crate::world::block_entity::BlockEntity;
            let mut proto = ProtoChunk::empty(pos, 1);
            let chest = blocks::get_block_id("minecraft:chest");
            proto.chunk.set_block(5, 10, 5, chest);
            proto.chunk.set_block_entity(BlockEntity::generated(
                [pos.x * 16 + 5, 10, pos.z * 16 + 5],
                "Chest",
                &[],
                None,
                None,
            ));
            proto
        }
        fn decorates(&self) -> bool {
            true
        }
        fn decorate(&self, region: &mut DecorationRegion<'_>) {
            use crate::world::block_entity::{BlockEntity, LootTable};
            let pos = region.origin();
            // Metadata-only origin write overwrites its proto actor before any blocks.
            region.set_block_entity(BlockEntity::generated(
                [pos.x * 16 + 5, 10, pos.z * 16 + 5],
                "Chest",
                &[],
                Some(LootTable::Bedrock("loot_tables/chests/test.json", -7)),
                None,
            ));
            // The negative neighbour supplies this chunk's top-height chest.
            let position = [pos.x * 16 + 16, 319, pos.z * 16 + 16];
            region.set_block(
                position[0],
                position[1],
                position[2],
                blocks::get_block_id("minecraft:chest"),
            );
            region.set_block_entity(BlockEntity::generated(position, "Chest", &[], None, None));
            // A later block replacement must clear its actor in final replay.
            let replaced = [pos.x * 16 + 6, -64, pos.z * 16 + 6];
            region.set_block(
                replaced[0],
                replaced[1],
                replaced[2],
                blocks::get_block_id("minecraft:chest"),
            );
            region.set_block_entity(BlockEntity::generated(replaced, "Chest", &[], None, None));
            region.set_block(replaced[0], replaced[1], replaced[2], *blocks::STONE);
        }
        fn block_classes(&self) -> &'static [u8] {
            &TEST_CLASSES
        }
        fn bedrock_biome(&self, biome: NativeBiome) -> u32 {
            biome as u32
        }
        fn find_spawn(&self, _chunk_at: &dyn Fn(ChunkPos) -> Chunk) -> SpawnPoint {
            SpawnPoint { x: 0, y: 11, z: 0 }
        }
    }

    fn actor_semantics(chunk: &Chunk) -> Vec<([i32; 3], zuri_nbt::NBTTag)> {
        let mut actors: Vec<_> = chunk
            .block_entities()
            .iter()
            .map(|entity| {
                let mut bytes = Vec::new();
                entity.write_disk_nbt(&mut bytes).unwrap();
                (
                    entity.position(),
                    zuri_nbt::NBTTag::read(bytes.as_slice(), zuri_nbt::encoding::LittleEndian)
                        .unwrap(),
                )
            })
            .collect();
        actors.sort_by_key(|(position, _)| *position);
        actors
    }

    #[test]
    fn pipeline_merges_proto_and_cached_neighbor_actors_independent_of_request_order() {
        let pos = ChunkPos::new(-1, -1);
        let next = ChunkPos::new(0, 0);
        let forward = ChunkPipeline::new(Arc::new(ActorGenerator));
        let a = forward.generate(pos);
        let b = forward.generate(next);
        let backward = ChunkPipeline::new(Arc::new(ActorGenerator));
        let b2 = backward.generate(next);
        let a2 = backward.generate(pos);
        assert_eq!(actor_semantics(&a), actor_semantics(&a2));
        assert_eq!(actor_semantics(&b), actor_semantics(&b2));
        assert_eq!(actor_semantics(&a), actor_semantics(&forward.generate(pos)));
        let positions: Vec<_> = actor_semantics(&a)
            .into_iter()
            .map(|(position, _)| position)
            .collect();
        assert_eq!(positions, vec![[-16, 319, -16], [-11, 10, -11]]);
        assert_eq!(
            a.get_block(0, 319, 0),
            blocks::get_block_id("minecraft:chest")
        );
        assert_eq!(a.get_block(6, -64, 6), *blocks::STONE);
        let (_, zuri_nbt::NBTTag::Compound(fields)) = &actor_semantics(&a)[1] else {
            panic!()
        };
        assert_eq!(
            fields.0.get("LootTableSeed"),
            Some(&zuri_nbt::NBTTag::Long((-7).into()))
        );
    }
}
