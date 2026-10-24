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
