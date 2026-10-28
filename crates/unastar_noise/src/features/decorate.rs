//! Java `ChunkGenerator.applyBiomeDecoration`: the per-chunk decoration loop.
//!
//! For one chunk, Java:
//! 1. seeds a `WorldgenRandom` (Xoroshiro) with
//!    `setDecorationSeed(levelSeed, minBlockX, minBlockZ)`;
//! 2. collects every biome present in the 3D biome storage of the chunk and its
//!    eight neighbours (restricted to the dimension's possible biomes);
//! 3. for each generation step: places the structures of that step (each with
//!    `setFeatureSeed(decorationSeed, structureIndex, step)`, handled by the
//!    structure layer through [`StepHook`]), then the union of the biomes'
//!    feature indices for the step in ascending order, each reseeded with
//!    `setFeatureSeed(decorationSeed, featureIndex, step)` and placed at the
//!    chunk's minimum corner (`x, minY, z`).

use super::blocks::BlockPos;
use super::eval::FeatureWorld;
use super::generated::{BIOME_COUNT, DimensionFeatures};
use super::random::FeatureRandom;

/// A set of biome indices (bit per index of `generated::BIOME_NAMES`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BiomeSet(pub u128);

impl BiomeSet {
    #[inline]
    pub fn insert(&mut self, biome: u8) {
        debug_assert!((biome as usize) < BIOME_COUNT);
        self.0 |= 1u128 << biome;
    }

    #[inline]
    pub fn contains(self, biome: u8) -> bool {
        self.0 & (1u128 << biome) != 0
    }

    /// Indices in ascending order.
    pub fn iter(self) -> impl Iterator<Item = u8> {
        (0..BIOME_COUNT as u8).filter(move |b| self.contains(*b))
    }
}

/// Called at the start of every generation step, before features, so the
/// structure layer can place the structure pieces of that step.
pub trait StepHook<W: FeatureWorld + ?Sized> {
    fn place_structures(
        &mut self,
        world: &mut W,
        rng: &mut FeatureRandom,
        decoration_seed: i64,
        step: usize,
    );
}

/// No structures (decoration of features only).
pub struct NoStructures;

impl<W: FeatureWorld + ?Sized> StepHook<W> for NoStructures {
    #[inline]
    fn place_structures(&mut self, _: &mut W, _: &mut FeatureRandom, _: i64, _: usize) {}
}

/// Decorate the chunk at (`chunk_x`, `chunk_z`).
///
/// `biomes` must hold every biome of the chunk and its eight neighbours that is
/// a possible biome of the dimension (Java's `retainAll(possibleBiomes)`).
pub fn decorate_chunk<W: FeatureWorld + ?Sized, H: StepHook<W>>(
    world: &mut W,
    dimension: &DimensionFeatures,
    chunk_x: i32,
    chunk_z: i32,
    biomes: BiomeSet,
    hook: &mut H,
) {
    let origin = BlockPos::new(chunk_x * 16, world.min_y(), chunk_z * 16);
    let mut rng = FeatureRandom::new(0);
    let decoration_seed = rng.set_decoration_seed(world.seed(), origin.x, origin.z);
    let mut indices: Vec<u16> = Vec::with_capacity(64);
    for step in 0..dimension.steps.len() {
        hook.place_structures(world, &mut rng, decoration_seed, step);
        let _stage = crate::profile::scope(crate::profile::Stage::decoration(step));
        indices.clear();
        for b in biomes.iter() {
            indices.extend_from_slice(dimension.biome_steps[b as usize][step]);
        }
        indices.sort_unstable();
        indices.dedup();
        for &i in &indices {
            rng.set_feature_seed(decoration_seed, i as i32, step as i32);
            let id = dimension.steps[step][i as usize];
            super::generated::place_placed(id, world, &mut rng, origin);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::generated::{OVERWORLD_FEATURES, biome_index};
    use crate::features::java_blocks::states;
    use crate::features::test_world::TestWorld;

    fn plains_world() -> TestWorld {
        let mut w = TestWorld::flat(63);
        w.biome = biome_index("minecraft:plains").unwrap();
        w.seed = 12345;
        for x in -16..32 {
            for z in -16..32 {
                w.blocks
                    .insert(BlockPos::new(x, 62, z), states::GRASS_BLOCK);
                w.blocks.insert(BlockPos::new(x, 61, z), states::DIRT);
            }
        }
        w.writable = Some((-16, -16, 31, 31));
        w
    }

    fn decorate(w: &mut TestWorld) {
        let mut set = BiomeSet::default();
        set.insert(w.biome);
        decorate_chunk(w, &OVERWORLD_FEATURES, 0, 0, set, &mut NoStructures);
    }

    /// A flat plains chunk gets ores underground and vegetation on the grass,
    /// and decoration is a pure function of the seed.
    #[test]
    fn plains_chunk_decorates_deterministically() {
        let mut a = plains_world();
        decorate(&mut a);
        let mut b = plains_world();
        decorate(&mut b);
        assert_eq!(a.blocks, b.blocks);
        assert!(a.count(states::COAL_ORE) > 0, "no coal ore");
        assert!(a.count(states::IRON_ORE) > 0, "no iron ore");
        let grass = a
            .blocks
            .values()
            .filter(|s| s.is(crate::features::java_blocks::blocks::SHORT_GRASS))
            .count();
        assert!(grass > 0, "no short grass");
        // Every write stayed in the writable 3x3 area.
        assert!(
            a.blocks
                .keys()
                .all(|p| (-16..32).contains(&p.x) && (-16..32).contains(&p.z))
        );
    }

    #[test]
    fn different_seeds_decorate_differently() {
        let mut a = plains_world();
        decorate(&mut a);
        let mut b = plains_world();
        b.seed = 54321;
        decorate(&mut b);
        assert_ne!(a.blocks, b.blocks);
    }

    /// Exercise the generated modifier chain and real ore implementation, not
    /// a copy of the height formula. Native inputs are isolated homogeneous
    /// worlds, so neighboring decoration history cannot affect this regression.
    #[test]
    fn placed_quartz_matches_native_height_context_and_rng() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/data/native_placed_quartz_1_21_11.json"
        ))
        .unwrap();
        let id = super::super::generated::PLACED_NAMES
            .iter()
            .position(|&name| name == "minecraft:ore_quartz_nether")
            .unwrap() as u16;
        for case in fixture["cases"].as_array().unwrap() {
            let seed = case["seed"].as_i64().unwrap();
            let cx = case["chunk_x"].as_i64().unwrap() as i32;
            let cz = case["chunk_z"].as_i64().unwrap() as i32;
            let mut world = TestWorld::flat(128);
            world.min_y = 0;
            world.height = 256;
            world.gen_depth = 128;
            world.fill = states::NETHERRACK;
            world.seed = seed;
            world.biome = biome_index("minecraft:nether_wastes").unwrap();
            let mut rng = FeatureRandom::new(0);
            let decoration_seed = rng.set_decoration_seed(seed, cx * 16, cz * 16);
            assert_eq!(decoration_seed, case["decoration_seed"].as_i64().unwrap());
            rng.set_feature_seed(
                decoration_seed,
                case["feature_index"].as_i64().unwrap() as i32,
                case["step"].as_i64().unwrap() as i32,
            );
            let placed = super::super::generated::place_placed(
                id,
                &mut world,
                &mut rng,
                BlockPos::new(cx * 16, 0, cz * 16),
            );
            assert_eq!(placed, case["success"].as_bool().unwrap());
            assert_eq!(
                rng.next_int(),
                case["next_int"].as_i64().unwrap() as i32,
                "seed {seed} chunk ({cx},{cz}) RNG continuation"
            );
            let expected: std::collections::HashMap<_, _> = case["blocks"]
                .as_array()
                .unwrap()
                .iter()
                .map(|block| {
                    assert_eq!(block["state"].as_str().unwrap(), "minecraft:nether_quartz_ore");
                    (
                        BlockPos::new(
                            block["x"].as_i64().unwrap() as i32,
                            block["y"].as_i64().unwrap() as i32,
                            block["z"].as_i64().unwrap() as i32,
                        ),
                        states::NETHER_QUARTZ_ORE,
                    )
                })
                .collect();
            assert_eq!(world.blocks, expected, "seed {seed} chunk ({cx},{cz})");
        }
    }
}
