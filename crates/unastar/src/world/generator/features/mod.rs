//! Java-style decoration of one origin chunk: runs the AOT-compiled Java
//! features and structure pieces (`unastar_noise::features`,
//! `unastar_noise::structures`) on a [`DecorationRegion`].
//!
//! The compiled code works in Java block-state space; [`RegionWorld`] converts
//! to and from Bedrock runtime ids through the shared Java->Bedrock table.
//! Current limits of the region (single block layer):
//! - waterlogged Java states are written as their Bedrock block without the
//!   water layer, and read back as not waterlogged;
//! - block entity data (chest loot tables and seeds, spawner mobs, template
//!   NBT) is produced but not stored in the chunk yet.

use std::sync::LazyLock;

use unastar_noise::features::decorate::{BiomeSet, decorate_chunk};
use unastar_noise::features::eval::BlockEntityData;
use unastar_noise::features::generated::{BIOME_NAMES, OVERWORLD_BIOMES, OVERWORLD_FEATURES};
use unastar_noise::features::java_blocks::states;
use unastar_noise::features::{BlockPos, FeatureWorld, Heightmap, JavaState};
use unastar_noise::java_biomes::JAVA_BIOMES;
use unastar_noise::java_blocks::JAVA_TO_BEDROCK;
use unastar_noise::random_hashes;
use unastar_noise::xoroshiro::{PositionalRandomFactory, Xoroshiro128};

use super::biome_source::{sample_biome, zoom_seed, zoomed_biome};
use super::density::NoiseRegistry;
use super::region::{DecorationRegion, HeightmapKind, SLOT_OFFSETS, WorldGenRegion};
use super::structures::JavaStructures;
use crate::world::chunk::MIN_Y;

/// Bedrock runtime id -> Java state id (the non-waterlogged state when several
/// Java states share one Bedrock state).
static JAVA_OF_BEDROCK: LazyLock<Vec<u16>> = LazyLock::new(|| {
    let max = JAVA_TO_BEDROCK.iter().copied().max().unwrap_or(0) as usize;
    let mut out = vec![u16::MAX; max + 1];
    for (java, &rid) in JAVA_TO_BEDROCK.iter().enumerate() {
        let slot = &mut out[rid as usize];
        let replace = *slot == u16::MAX
            || (JavaState(*slot).value("waterlogged") == Some("true")
                && JavaState(java as u16).value("waterlogged") != Some("true"));
        if replace {
            *slot = java as u16;
        }
    }
    out
});

/// Both biome tables are sorted by name; the compiled feature code indexes the
/// same list the biome source does.
static BIOMES_ALIGNED: LazyLock<()> = LazyLock::new(|| {
    assert_eq!(JAVA_BIOMES.len(), BIOME_NAMES.len(), "biome tables differ");
    for (a, b) in JAVA_BIOMES.iter().zip(BIOME_NAMES.iter()) {
        assert_eq!(a.name, *b, "biome tables are not in the same order");
    }
});

#[inline]
fn to_bedrock(state: JavaState) -> u32 {
    JAVA_TO_BEDROCK[state.0 as usize]
}

#[inline]
fn to_java(rid: u32) -> JavaState {
    match JAVA_OF_BEDROCK.get(rid as usize) {
        Some(&s) if s != u16::MAX => JavaState(s),
        _ => states::AIR,
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
pub struct RegionWorld<'r, 'a> {
    region: &'r mut DecorationRegion<'a>,
    noises: &'r NoiseRegistry,
    zoom_seed: i64,
    region_random: Xoroshiro128,
    origin: (i32, i32),
}

impl<'r, 'a> RegionWorld<'r, 'a> {
    pub fn new(region: &'r mut DecorationRegion<'a>, noises: &'r NoiseRegistry) -> Self {
        LazyLock::force(&BIOMES_ALIGNED);
        let seed = region.seed();
        let origin = region.origin();
        // `randomState.getOrCreateRandomFactory(worldgen_region_random).at(chunk origin)`.
        let region_random = PositionalRandomFactory::new(seed)
            .fork_from_hash(random_hashes::WORLDGEN_REGION_RANDOM)
            .at(origin.x * 16, 0, origin.z * 16);
        Self {
            region,
            noises,
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
            sample_biome(self.noises, qx, qy, qz) as u8
        }
    }
}

impl FeatureWorld for RegionWorld<'_, '_> {
    fn min_y(&self) -> i32 {
        MIN_Y
    }
    fn height(&self) -> i32 {
        self.region.height()
    }
    fn sea_level(&self) -> i32 {
        unastar_noise::overworld_settings::SEA_LEVEL
    }
    fn seed(&self) -> i64 {
        self.region.seed()
    }
    fn get_state(&mut self, pos: BlockPos) -> JavaState {
        if !self.inside(pos.x, pos.z) {
            return states::AIR;
        }
        to_java(self.region.get_block(pos.x, pos.y, pos.z))
    }
    fn set_state(&mut self, pos: BlockPos, state: JavaState) -> bool {
        self.region
            .set_block(pos.x, pos.y, pos.z, to_bedrock(state))
    }
    fn get_height(&mut self, kind: Heightmap, x: i32, z: i32) -> i32 {
        if !self.inside(x, z) {
            return MIN_Y;
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
    fn region_random(&mut self) -> &mut Xoroshiro128 {
        &mut self.region_random
    }
    fn gen_depth(&self) -> i32 {
        unastar_noise::overworld_settings::HEIGHT
    }
    fn set_block_entity(&mut self, _pos: BlockPos, _data: BlockEntityData) {}
}

/// Decorate the region's origin chunk: Java features and structure pieces.
pub fn decorate(
    seed: i64,
    noises: &NoiseRegistry,
    structures: &JavaStructures,
    region: &mut DecorationRegion<'_>,
) {
    let origin = region.origin();
    // Biomes of the 3x3 chunks' sections, limited to the overworld's biomes.
    let mut biomes = BiomeSet::default();
    let quarts_y = region.height() >> 2;
    for (dx, dz) in SLOT_OFFSETS {
        let (qx0, qz0) = ((origin.x + dx) * 4, (origin.z + dz) * 4);
        for qy in 0..quarts_y {
            for qx in qx0..qx0 + 4 {
                for qz in qz0..qz0 + 4 {
                    let b = region.biome_at(qx, (MIN_Y >> 2) + qy, qz) as u8;
                    if OVERWORLD_BIOMES & (1u128 << b) != 0 {
                        biomes.insert(b);
                    }
                }
            }
        }
    }
    let ctx = structures.context(seed, noises);
    let mut hook = structures.manager.decoration_hook(&ctx, origin.x, origin.z);
    let mut world = RegionWorld::new(region, noises);
    decorate_chunk(
        &mut world,
        &OVERWORLD_FEATURES,
        origin.x,
        origin.z,
        biomes,
        &mut hook,
    );
}
