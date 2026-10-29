//! Java-style decoration of one origin chunk: runs the AOT-compiled Java
//! features and structure pieces (`unastar_noise::features`,
//! `unastar_noise::structures`) on a [`DecorationRegion`].
//!
//! Every read and write remains in canonical Java block-state space. The chunk
//! pipeline performs the intentionally lossy Bedrock conversion only after all
//! features and structures have finished observing and transforming native states.

use std::sync::LazyLock;

use unastar_noise::features::decorate::{BiomeSet, NoStructures, decorate_chunk};
use unastar_noise::features::eval::BlockEntityData;
use unastar_noise::features::generated::{
    BIOME_NAMES, DimensionFeatures, END_BIOMES, END_FEATURES, NETHER_BIOMES, NETHER_FEATURES,
    OVERWORLD_BIOMES, OVERWORLD_FEATURES,
};
use unastar_noise::features::java_blocks::{self, states};
use unastar_noise::features::random::RegionRandom;
use unastar_noise::features::{BlockPos, FeatureWorld, Heightmap, JavaBlock, JavaState};
use unastar_noise::java_biomes::JAVA_BIOMES;

use super::JavaDimension;
use super::biome_source::{zoom_seed, zoomed_biome};
use super::region::{DecorationRegion, HeightmapKind, SLOT_OFFSETS, WorldGenRegion};
use super::structures::{JavaStructures, NativeStructureSource};
use crate::world::block_entity::{BlockEntity, NATIVE_ACTOR_BLOCKS};
use crate::world::chunk::BlockStateSpace;

/// The native registry's EntityBlock predicate, indexed by canonical block
/// identity rather than properties or a lossy Bedrock actor/state mapping.
static HAS_NATIVE_ACTOR: LazyLock<[bool; java_blocks::BLOCKS.len()]> = LazyLock::new(|| {
    let mut flags = [false; java_blocks::BLOCKS.len()];
    for &(name, _) in NATIVE_ACTOR_BLOCKS {
        let block = JavaBlock::by_name(name).expect("pinned native actor block is absent");
        flags[usize::from(block.0)] = true;
    }
    flags
});

fn has_native_actor(state: JavaState) -> bool {
    HAS_NATIVE_ACTOR[usize::from(state.block().0)]
}

/// Both biome tables are sorted by name; the compiled feature code indexes the
/// same list the biome source does.
static BIOMES_ALIGNED: LazyLock<()> = LazyLock::new(|| {
    assert_eq!(JAVA_BIOMES.len(), BIOME_NAMES.len(), "biome tables differ");
    for (a, b) in JAVA_BIOMES.iter().zip(BIOME_NAMES.iter()) {
        assert_eq!(a.name, *b, "biome tables are not in the same order");
    }
});

/// Compiled decoration and native generator settings selected once per origin.
pub(super) struct DimensionDecoration {
    pub features: &'static DimensionFeatures,
    pub biomes: u128,
    pub min_y: i32,
    pub gen_depth: i32,
    pub sea_level: i32,
    pub legacy_random: bool,
    pub has_skylight: bool,
}

pub(super) fn dimension_decoration(dimension: JavaDimension) -> DimensionDecoration {
    use unastar_noise::{end_settings as end, nether_settings as nether, overworld_settings as ow};
    let (features, biomes, min_y, gen_depth, sea_level, legacy_random, has_skylight) =
        match dimension {
            JavaDimension::Overworld => (
                &OVERWORLD_FEATURES,
                OVERWORLD_BIOMES,
                ow::MIN_Y,
                ow::HEIGHT,
                ow::SEA_LEVEL,
                ow::LEGACY_RANDOM_SOURCE,
                ow::HAS_SKYLIGHT,
            ),
            JavaDimension::Nether => (
                &NETHER_FEATURES,
                NETHER_BIOMES,
                nether::MIN_Y,
                nether::HEIGHT,
                nether::SEA_LEVEL,
                nether::LEGACY_RANDOM_SOURCE,
                nether::HAS_SKYLIGHT,
            ),
            JavaDimension::End => (
                &END_FEATURES,
                END_BIOMES,
                end::MIN_Y,
                end::HEIGHT,
                end::SEA_LEVEL,
                end::LEGACY_RANDOM_SOURCE,
                end::HAS_SKYLIGHT,
            ),
        };
    DimensionDecoration {
        features,
        biomes,
        min_y,
        gen_depth,
        sea_level,
        legacy_random,
        has_skylight,
    }
}

fn kind_of(h: Heightmap) -> HeightmapKind {
    match h {
        Heightmap::WorldSurfaceWg => HeightmapKind::WorldSurfaceWg,
        Heightmap::WorldSurface => HeightmapKind::WorldSurface,
        Heightmap::OceanFloorWg => HeightmapKind::OceanFloorWg,
        Heightmap::OceanFloor => HeightmapKind::OceanFloor,
        Heightmap::MotionBlocking => HeightmapKind::MotionBlocking,
        Heightmap::MotionBlockingNoLeaves => HeightmapKind::MotionBlockingNoLeaves,
    }
}

/// [`FeatureWorld`] over a decoration region.
pub struct RegionWorld<'r, 'a, S: NativeStructureSource> {
    region: &'r mut DecorationRegion<'a>,
    source: &'r S,
    settings: DimensionDecoration,
    zoom_seed: i64,
    region_random: RegionRandom,
    origin: (i32, i32),
}

impl<'r, 'a, S: NativeStructureSource> RegionWorld<'r, 'a, S> {
    pub fn new(
        region: &'r mut DecorationRegion<'a>,
        dimension: JavaDimension,
        source: &'r S,
    ) -> Self {
        assert_eq!(
            region.state_space(),
            BlockStateSpace::Java,
            "Java features require source-native Java protos, not an inverse Bedrock mapping"
        );
        LazyLock::force(&BIOMES_ALIGNED);
        let seed = region.seed();
        let origin = region.origin();
        let settings = dimension_decoration(dimension);
        let region_random = RegionRandom::new(seed, settings.legacy_random, origin.x, origin.z);
        Self {
            region,
            source,
            settings,
            zoom_seed: zoom_seed(seed),
            region_random,
            origin: (origin.x, origin.z),
        }
    }

    #[inline]
    fn inside(&self, x: i32, z: i32) -> bool {
        let (dx, dz) = ((x >> 4) - self.origin.0, (z >> 4) - self.origin.1);
        (-1..=1).contains(&dx) && (-1..=1).contains(&dz)
    }

    /// Noise biome of a quart: from the region's protos, or the biome source
    /// outside them (Java's uncached fallback).
    fn quart_biome(&self, qx: i32, qy: i32, qz: i32) -> u8 {
        if self.inside(qx << 2, qz << 2) {
            self.region.biome_at(qx, qy, qz) as u8
        } else {
            self.source.noise_biome(qx, qy, qz)
        }
    }
}

impl<S: NativeStructureSource> FeatureWorld for RegionWorld<'_, '_, S> {
    fn min_y(&self) -> i32 {
        self.region.min_y()
    }
    fn height(&self) -> i32 {
        self.region.height()
    }
    fn sea_level(&self) -> i32 {
        self.settings.sea_level
    }
    fn raw_brightness(&mut self, _pos: BlockPos, sky_darken: i32) -> i32 {
        // This region only runs pre-INITIALIZE_LIGHT generation. Native visible
        // section storage is still virgin: absent block light is zero and
        // absent sky light is fifteen, independent of placed roofs or terrain.
        if self.settings.has_skylight {
            15_i32.wrapping_sub(sky_darken).max(0)
        } else {
            0
        }
    }
    fn seed(&self) -> i64 {
        self.region.seed()
    }
    fn get_state(&mut self, pos: BlockPos) -> JavaState {
        if !self.inside(pos.x, pos.z) {
            return states::AIR;
        }
        if !(self.region.min_y()..self.region.min_y() + self.region.height()).contains(&pos.y) {
            return states::VOID_AIR;
        }
        JavaState(
            u16::try_from(self.region.get_block(pos.x, pos.y, pos.z))
                .expect("Java feature region contains an invalid state id"),
        )
    }
    fn set_state(&mut self, pos: BlockPos, state: JavaState) -> bool {
        if !self.inside(pos.x, pos.z) {
            return false;
        }
        let previous = self.get_state(pos);
        let creates_actor = has_native_actor(state);
        // ProtoChunk state writes do not destroy an instantiated actor, even
        // when another EntityBlock replaces its block type. Chunk's ordinary
        // state setter has stronger removal rules, so retain that native actor
        // across this write. A pending marker, however, is always replaced.
        let live = if creates_actor && previous.block() != state.block() {
            self.region
                .block_entity(pos.x, pos.y, pos.z)
                .filter(|actor| !actor.is_java_pending())
                .cloned()
        } else {
            None
        };
        if (self.region.min_y()..self.region.min_y() + self.region.height()).contains(&pos.y) {
            self.region
                .set_block(pos.x, pos.y, pos.z, u32::from(state.0));
        }
        if creates_actor {
            if let Some(actor) = live {
                self.region.set_native_block_entity(actor);
            } else if self
                .region
                .block_entity(pos.x, pos.y, pos.z)
                .is_none_or(BlockEntity::is_java_pending)
            {
                self.region
                    .set_native_block_entity(BlockEntity::pending_java(
                        [pos.x, pos.y, pos.z],
                        state.block().name(),
                    ));
            }
        } else if has_native_actor(previous) {
            self.region.remove_block_entity(pos.x, pos.y, pos.z);
        }
        // WorldGenRegion only bounds X/Z for ordinary protos. Outside build
        // height ProtoChunk leaves the state unchanged but may still queue NBT.
        true
    }
    fn get_height(&mut self, kind: Heightmap, x: i32, z: i32) -> i32 {
        if !self.inside(x, z) {
            return self.region.min_y();
        }
        self.region.heightmap(kind_of(kind), x, z)
    }
    fn biome(&mut self, pos: BlockPos) -> u8 {
        let seed = self.zoom_seed;
        zoomed_biome(seed, pos.x, pos.y, pos.z, |qx, qy, qz| {
            self.quart_biome(qx, qy, qz)
        })
    }
    fn schedule_tick(&mut self, _pos: BlockPos) {}
    fn region_random(&mut self) -> &mut RegionRandom {
        &mut self.region_random
    }
    fn gen_depth(&self) -> i32 {
        self.settings.gen_depth
    }
    fn set_block_entity(&mut self, pos: BlockPos, data: BlockEntityData) {
        if !self.inside(pos.x, pos.z) || !has_native_actor(self.get_state(pos)) {
            return;
        }
        let actor = if data.template_nbt.is_none() {
            let Some(actor) = self.region.block_entity(pos.x, pos.y, pos.z) else {
                return;
            };
            actor.with_java_feature_data(data)
        } else {
            BlockEntity::generated_java(
                [pos.x, pos.y, pos.z],
                self.get_state(pos).block().name(),
                data,
            )
        };
        self.region.set_block_entity(actor);
    }
}

/// Decorate the region's origin chunk: Java features and structure pieces.
pub fn decorate<S: NativeStructureSource>(
    seed: i64,
    dimension: JavaDimension,
    structures: &JavaStructures,
    source: &S,
    region: &mut DecorationRegion<'_>,
    generate_structures: bool,
    biome_features: bool,
) {
    let origin = region.origin();
    let settings = dimension_decoration(dimension);
    // Java retains the source's possible biomes after collecting all stored sections.
    let mut biomes = BiomeSet::default();
    let quarts_y = region.height() >> 2;
    if biome_features {
        for (dx, dz) in SLOT_OFFSETS {
            let (qx0, qz0) = ((origin.x + dx) * 4, (origin.z + dz) * 4);
            for qy in 0..quarts_y {
                for qx in qx0..qx0 + 4 {
                    for qz in qz0..qz0 + 4 {
                        let b = region.biome_at(qx, (region.min_y() >> 2) + qy, qz) as u8;
                        if settings.biomes & (1u128 << b) != 0 {
                            biomes.insert(b);
                        }
                    }
                }
            }
        }
    }
    let mut world = RegionWorld::new(region, dimension, source);
    if generate_structures {
        let ctx = structures.context(seed, source);
        let mut hook = structures.manager.decoration_hook(&ctx, origin.x, origin.z);
        decorate_chunk(
            &mut world,
            settings.features,
            origin.x,
            origin.z,
            biomes,
            &mut hook,
        );
    } else {
        decorate_chunk(
            &mut world,
            settings.features,
            origin.x,
            origin.z,
            biomes,
            &mut NoStructures,
        );
    }
}

#[cfg(test)]
mod state_identity_tests {
    use super::*;
    use crate::world::ChunkPos;
    use crate::world::generator::ProtoChunk;

    #[test]
    fn placed_waterlogged_state_remains_source_fluid_for_later_features() {
        std::thread::Builder::new()
            .stack_size(64 << 20)
            .spawn(|| {
                let origin = ChunkPos::new(-1, -1);
                let protos = SLOT_OFFSETS.map(|(dx, dz)| {
                    ProtoChunk::empty_in_space(
                        ChunkPos::new(origin.x + dx, origin.z + dz),
                        0,
                        BlockStateSpace::Java,
                    )
                });
                let refs = std::array::from_fn(|slot| &protos[slot]);
                let mut region = DecorationRegion::new(
                    0,
                    origin,
                    refs,
                    &unastar_noise::java_blocks::JAVA_STATE_CLASSES,
                );
                let source = crate::world::generator::JavaGenerator::new(0);
                let mut world = RegionWorld::new(&mut region, JavaDimension::Overworld, &source);
                let position = BlockPos::new(-1, 64, -1);
                let wet = states::OAK_STAIRS.with_bool("waterlogged", true);
                assert!(world.set_state(position, wet));
                let fluid = world.get_state(position).fluid();
                assert!(
                    fluid.source,
                    "later features must observe the placed source water"
                );
                assert_eq!(
                    fluid.kind,
                    unastar_noise::features::blocks::FluidKind::Water
                );
            })
            .unwrap()
            .join()
            .unwrap();
    }

    fn native_actor_fixture() -> serde_json::Value {
        serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/java_1_21_11_native_actor_transitions.json",
        )))
        .unwrap()
    }

    fn captured_state(text: &str) -> JavaState {
        let (name, properties) = text.split_once('[').map_or((text, ""), |(name, rest)| {
            (name, rest.strip_suffix(']').unwrap())
        });
        let block = JavaBlock::by_name(name).unwrap();
        let mut state = block.default_state();
        let mut count = 0;
        for property in properties.split(',').filter(|p| !p.is_empty()) {
            let (name, value) = property.split_once('=').unwrap();
            state = state.with(name, value);
            count += 1;
        }
        assert_eq!(
            count,
            block.def().props.len(),
            "capture must retain every property"
        );
        state
    }

    fn captured_barrel_template() -> BlockEntityData {
        use unastar_noise::structures::native_actor::{Tag, TemplateActor, key, string};
        static CAPTURE: LazyLock<(Vec<u8>, Vec<u8>)> = LazyLock::new(|| {
            let source = Tag::Compound(std::collections::BTreeMap::from([
                (key("id"), string("minecraft:barrel")),
                (
                    key("LootTable"),
                    string("minecraft:chests/shipwreck_supply"),
                ),
                (key("LootTableSeed"), Tag::Long(17)),
            ]))
            .write_root()
            .unwrap();
            // Native loadStatic adds only components to this source;
            // position is supplied by the runtime recipe.
            let normalization = Tag::Compound(std::collections::BTreeMap::from([
                (key("remove"), Tag::List(8, vec![])),
                (
                    key("set"),
                    Tag::Compound(std::collections::BTreeMap::from([(
                        key("components"),
                        Tag::Compound(Default::default()),
                    )])),
                ),
            ]))
            .write_root()
            .unwrap();
            (source, normalization)
        });
        BlockEntityData {
            loot: None,
            spawner_entity: None,
            template_nbt: Some(TemplateActor {
                id: "minecraft:barrel",
                source: &CAPTURE.0,
                normalization: &CAPTURE.1,
            }),
        }
    }

    #[test]
    fn native_proto_actor_transitions_match_actual_worldgen_region() {
        std::thread::Builder::new()
            .stack_size(64 << 20)
            .spawn(|| {
                let fixture = native_actor_fixture();
                assert_eq!(fixture["native_tick_count"], 0);
                let origin = ChunkPos::new(0, 0);
                let protos = SLOT_OFFSETS.map(|(dx, dz)| {
                    ProtoChunk::empty_in_space(ChunkPos::new(dx, dz), 0, BlockStateSpace::Java)
                });
                let refs = std::array::from_fn(|slot| &protos[slot]);
                let mut region = DecorationRegion::new(
                    0,
                    origin,
                    refs,
                    &unastar_noise::java_blocks::JAVA_STATE_CLASSES,
                );
                let source = crate::world::generator::JavaGenerator::new(0);
                let mut world = RegionWorld::new(&mut region, JavaDimension::Overworld, &source);
                let mut pos = BlockPos::new(1, 200, 1);
                for name in fixture["actor_blocks"].as_array().unwrap() {
                    let state = JavaBlock::by_name(name.as_str().unwrap())
                        .unwrap()
                        .default_state();
                    assert!(world.set_state(pos, state));
                    let actor = world.region.block_entity(pos.x, pos.y, pos.z).unwrap();
                    assert!(actor.is_java_pending(), "{}", name);
                    assert_eq!(
                        actor.canonical_java_nbt().unwrap(),
                        fixture["transitions"][0]["saved"]
                    );
                }
                for expected in fixture["transitions"].as_array().unwrap() {
                    let operation = expected["operation"].as_str().unwrap();
                    match operation {
                        "explicit_loot" => world.set_block_entity(
                            pos,
                            BlockEntityData::loot("minecraft:chests/simple_dungeon", i64::MIN),
                        ),
                        "explicit_loot_after_actor_replacement" => world.set_block_entity(
                            pos,
                            BlockEntityData::loot("minecraft:chests/simple_dungeon", 0),
                        ),
                        "explicit_template" => {
                            world.set_block_entity(pos, captured_barrel_template())
                        }
                        "explicit_template_over_live" => {
                            let mut data = captured_barrel_template();
                            data.loot = Some(("minecraft:chests/shipwreck_supply", -29));
                            world.set_block_entity(pos, data);
                        }
                        "above_bounds_chest:true" => {
                            pos.y = 320;
                            assert!(world.set_state(pos, states::CHEST));
                        }
                        _ => {
                            assert!(world.set_state(
                                pos,
                                captured_state(expected["state"].as_str().unwrap())
                            ))
                        }
                    }
                    assert_eq!(
                        world.get_state(pos),
                        captured_state(expected["state"].as_str().unwrap()),
                        "{operation}"
                    );
                    let actual = world.region.block_entity(pos.x, pos.y, pos.z);
                    assert_eq!(
                        actual.map(|a| a.canonical_java_nbt().unwrap()),
                        (!expected["saved"].is_null()).then(|| expected["saved"].clone()),
                        "{operation}"
                    );
                    if let Some(actor) = actual {
                        assert_eq!(
                            actor.is_java_pending(),
                            expected["pending"] == 1,
                            "{operation}"
                        );
                    }
                }
                let mut replay =
                    crate::world::chunk::Chunk::new_in_space(0, 0, BlockStateSpace::Java);
                region.into_writes().apply_to(0, 0, &mut replay);
                assert_eq!(replay.block_entities().len(), 1);
                assert_eq!(
                    replay.block_entities()[0].canonical_java_nbt().unwrap(),
                    fixture["transitions"].as_array().unwrap().last().unwrap()["saved"]
                );
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn actual_features_pending_sculk_actors_reach_canonical_consumer_unchanged() {
        std::thread::Builder::new()
            .stack_size(64 << 20)
            .spawn(|| {
                let fixture = native_actor_fixture();
                let capture = &fixture["features_witnesses"];
                let seed = capture["seed"].as_i64().unwrap();
                let origin = ChunkPos::new(
                    capture["chunk"][0].as_i64().unwrap() as i32,
                    capture["chunk"][1].as_i64().unwrap() as i32,
                );
                let protos = SLOT_OFFSETS.map(|(dx, dz)| {
                    ProtoChunk::empty_in_space(
                        ChunkPos::new(origin.x + dx, origin.z + dz),
                        0,
                        BlockStateSpace::Java,
                    )
                });
                let refs = std::array::from_fn(|slot| &protos[slot]);
                let mut region = DecorationRegion::new(
                    seed,
                    origin,
                    refs,
                    &unastar_noise::java_blocks::JAVA_STATE_CLASSES,
                );
                let source = crate::world::generator::JavaGenerator::new(seed);
                let mut world = RegionWorld::new(&mut region, JavaDimension::Overworld, &source);
                let witnesses = capture["actors"].as_array().unwrap();
                for expected in witnesses {
                    let xyz = expected["position"].as_array().unwrap();
                    let pos = BlockPos::new(
                        xyz[0].as_i64().unwrap() as i32,
                        xyz[1].as_i64().unwrap() as i32,
                        xyz[2].as_i64().unwrap() as i32,
                    );
                    let state = captured_state(expected["state"].as_str().unwrap());
                    assert!(world.set_state(pos, state));
                    assert_eq!(world.get_state(pos), state);
                }
                let mut chunk = crate::world::chunk::Chunk::new_in_space(
                    origin.x,
                    origin.z,
                    BlockStateSpace::Java,
                );
                region.into_writes().apply_to(0, 0, &mut chunk);
                assert_eq!(chunk.block_entities().len(), witnesses.len());
                for expected in witnesses {
                    let xyz = expected["position"].as_array().unwrap();
                    let position = [
                        xyz[0].as_i64().unwrap() as i32,
                        xyz[1].as_i64().unwrap() as i32,
                        xyz[2].as_i64().unwrap() as i32,
                    ];
                    let actor = chunk
                        .block_entities()
                        .iter()
                        .find(|a| a.position() == position)
                        .unwrap();
                    assert_eq!(
                        actor.canonical_java_nbt().unwrap(),
                        expected["canonical_nbt"]
                    );
                    assert!(
                        actor.write_disk_nbt(&mut Vec::new()).is_err(),
                        "no early Bedrock conversion"
                    );
                }
            })
            .unwrap()
            .join()
            .unwrap();
    }
}
