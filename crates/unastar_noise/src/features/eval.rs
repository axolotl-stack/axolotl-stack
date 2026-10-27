//! The world view decoration runs against ([`FeatureWorld`]) and the generic
//! evaluators for data-driven predicates, rule tests and state providers.

use super::blocks::{
    BlockPos, BlockPredicate, Direction, Fluid, Heightmap, JavaState, RuleBasedStateProvider,
    RuleTest, StateProvider,
};
use super::random::{BitSource, FeatureRandom, WorldgenRandom};
use super::values::weighted_pick;

/// Block entity contents produced by generation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlockEntityData {
    /// Loot table id (`minecraft:chests/simple_dungeon`) with its seed.
    pub loot: Option<(&'static str, i64)>,
    /// Spawner / trial spawner entity id (`minecraft:zombie`).
    pub spawner_entity: Option<&'static str>,
    /// Full Java template identity, typed source NBT and native save normalization.
    pub template_nbt: Option<crate::structures::native_actor::TemplateActor>,
}

impl BlockEntityData {
    /// Loot table only.
    pub const fn loot(table: &'static str, seed: i64) -> Self {
        Self {
            loot: Some((table, seed)),
            spawner_entity: None,
            template_nbt: None,
        }
    }

    /// Spawner entity only.
    pub const fn spawner(entity: &'static str) -> Self {
        Self {
            loot: None,
            spawner_entity: Some(entity),
            template_nbt: None,
        }
    }
}
/// Java `WorldGenLevel` as seen by features, in Java block-state space.
///
/// Implemented by the generator runtime over the 3x3 decoration region. Reads
/// outside the region return air; writes outside the writable area are
/// dropped and report `false`, like Java's `WorldGenRegion.ensureCanWrite`.
pub trait FeatureWorld {
    /// Lowest buildable Y.
    fn min_y(&self) -> i32;
    /// Build height (number of layers).
    fn height(&self) -> i32;
    /// Sea level of the dimension's noise settings.
    fn sea_level(&self) -> i32;
    /// Native dimension skylight capability, independent of noise-fill height.
    fn has_skylight(&self) -> bool;
    /// Level seed.
    fn seed(&self) -> i64;

    /// Block state at `pos` (waterlogged states included).
    fn get_state(&mut self, pos: BlockPos) -> JavaState;
    /// `setBlock(pos, state, flags)`; `false` if `pos` is not writable.
    fn set_state(&mut self, pos: BlockPos, state: JavaState) -> bool;
    /// `getHeight(type, x, z)`: one above the highest opaque block, live.
    fn get_height(&mut self, kind: Heightmap, x: i32, z: i32) -> i32;
    /// Biome at a block (Java `getBiome(pos)`, with the seed-based zoom),
    /// as an index into the generated biome list.
    fn biome(&mut self, pos: BlockPos) -> u8;
    /// `scheduleTick(pos, block, 0)` request for blocks placed with
    /// `schedule_tick` (fluids, falling blocks). Runtimes that do not tick
    /// during generation may ignore it.
    fn schedule_tick(&mut self, pos: BlockPos);
    /// The region's native random (`WorldGenRegion.getRandom()`), constructed
    /// from the dimension's positional factory at the chunk's minimum block
    /// position. Overworld uses native Xoroshiro draws; Nether/End use legacy
    /// draws. This is deliberately not the decoration `WorldgenRandom`.
    fn region_random(&mut self) -> &mut super::random::RegionRandom;
    /// `ChunkGenerator.getGenDepth()`: the noise settings' `height` (384 in the
    /// overworld, 128 in the nether), which can differ from [`Self::height`].
    fn gen_depth(&self) -> i32;
    /// Block entity data for a block just placed at `pos` (chest loot tables,
    /// spawner entities, template block entity NBT). The runtime materialises
    /// it for the output format; it does not affect generation.
    fn set_block_entity(&mut self, pos: BlockPos, data: BlockEntityData);

    // ---- derived helpers -------------------------------------------------

    /// Highest Y + 1.
    #[inline]
    fn max_y(&self) -> i32 {
        self.min_y() + self.height()
    }

    /// `isOutsideBuildHeight`.
    #[inline]
    fn is_outside_build_height(&self, y: i32) -> bool {
        y < self.min_y() || y >= self.max_y()
    }

    #[inline]
    fn is_air(&mut self, pos: BlockPos) -> bool {
        self.get_state(pos).is_air()
    }

    #[inline]
    fn fluid(&mut self, pos: BlockPos) -> Fluid {
        self.get_state(pos).fluid()
    }

    /// `isStateAtPosition(pos, s -> s.is(block))`.
    #[inline]
    fn is_block(&mut self, pos: BlockPos, block: super::blocks::JavaBlock) -> bool {
        self.get_state(pos).is(block)
    }

    /// `getHeightmapPos(type, pos)`.
    #[inline]
    fn heightmap_pos(&mut self, kind: Heightmap, pos: BlockPos) -> BlockPos {
        let y = self.get_height(kind, pos.x, pos.z);
        pos.with_y(y)
    }
}

/// Evaluate a [`BlockPredicate`] at `pos`.
pub fn test_predicate<W: FeatureWorld + ?Sized>(
    world: &mut W,
    pred: &BlockPredicate,
    pos: BlockPos,
) -> bool {
    match *pred {
        BlockPredicate::MatchingBlocks { set, offset } => {
            world.get_state(at(pos, offset)).is_in(set)
        }
        BlockPredicate::MatchingFluids { fluids, offset } => {
            fluids.contains(world.get_state(at(pos, offset)).fluid())
        }
        BlockPredicate::Solid { offset } => world.get_state(at(pos, offset)).is_solid(),
        BlockPredicate::Replaceable { offset } => {
            world.get_state(at(pos, offset)).can_be_replaced()
        }
        BlockPredicate::WouldSurvive { state, offset } => {
            super::survival::can_survive(world, state, at(pos, offset))
        }
        BlockPredicate::HasSturdyFace { direction, offset } => {
            world.get_state(at(pos, offset)).is_face_sturdy(direction)
        }
        BlockPredicate::InsideWorldBounds { offset } => {
            !world.is_outside_build_height(at(pos, offset).y)
        }
        BlockPredicate::AllOf(preds) => preds.iter().all(|p| test_predicate(world, p, pos)),
        BlockPredicate::AnyOf(preds) => preds.iter().any(|p| test_predicate(world, p, pos)),
        BlockPredicate::Not(p) => !test_predicate(world, p, pos),
        BlockPredicate::True => true,
    }
}

#[inline]
fn at(pos: BlockPos, offset: [i32; 3]) -> BlockPos {
    pos.offset(offset[0], offset[1], offset[2])
}

/// Evaluate a [`RuleTest`] against a state.
pub fn test_rule<S: BitSource>(
    rule: &RuleTest,
    state: JavaState,
    rng: &mut WorldgenRandom<S>,
) -> bool {
    match *rule {
        RuleTest::AlwaysTrue => true,
        RuleTest::BlockMatch(block) => state.is(block),
        RuleTest::BlockStateMatch(s) => state == s,
        RuleTest::TagMatch(set) => state.is_in(set),
        RuleTest::RandomBlockMatch(block, p) => state.is(block) && rng.next_float() < p,
        RuleTest::RandomBlockStateMatch(s, p) => state == s && rng.next_float() < p,
    }
}

/// `Direction.Axis.getRandom(random)`.
#[inline]
pub fn random_axis<S: BitSource>(rng: &mut WorldgenRandom<S>) -> &'static str {
    ["x", "y", "z"][rng.next_int_bounded(3) as usize]
}

/// `BlockStateProvider.getState(random, pos)`.
pub fn provide_state(
    provider: &StateProvider,
    rng: &mut FeatureRandom,
    pos: BlockPos,
) -> JavaState {
    match *provider {
        StateProvider::Simple(s) => s,
        StateProvider::Rotated(s) => s.with("axis", random_axis(rng)),
        StateProvider::Weighted {
            entries,
            total_weight,
        } => *weighted_pick(entries, total_weight, rng),
        StateProvider::RandomizedInt {
            source,
            property,
            values,
        } => {
            let s = provide_state(source, rng, pos);
            if !s.has(property) {
                return s;
            }
            let v = values.sample(rng);
            s.with_int(property, v)
        }
        StateProvider::Noise {
            noise,
            scale,
            states,
        } => {
            let v = noise_value(noise, pos, scale);
            pick_by_noise(states, v)
        }
        StateProvider::DualNoise {
            noise,
            slow_noise,
            scale,
            slow_scale,
            variety,
            states,
        } => {
            let slow = noise_value(slow_noise, pos, slow_scale);
            let count =
                clamped_map(slow, -1.0, 1.0, variety.0 as f64, (variety.1 + 1) as f64) as i32;
            // Java builds the candidate list then picks from it with the main noise.
            let mut candidates = [JavaState(0); 32];
            let count = count.clamp(0, candidates.len() as i32) as usize;
            for (j, slot) in candidates.iter_mut().enumerate().take(count) {
                let offset = pos.offset(j as i32 * 54545, 0, j as i32 * 34234);
                *slot = pick_by_noise(states, noise_value(noise, offset, scale));
            }
            pick_by_noise(&candidates[..count], noise_value(noise, pos, scale))
        }
        StateProvider::NoiseThreshold {
            noise,
            scale,
            threshold,
            high_chance,
            default_state,
            low_states,
            high_states,
        } => {
            let v = noise_value(noise, pos, scale);
            if v < threshold as f64 {
                low_states[rng.next_int_bounded(low_states.len() as i32) as usize]
            } else if rng.next_float() < high_chance {
                high_states[rng.next_int_bounded(high_states.len() as i32) as usize]
            } else {
                default_state
            }
        }
    }
}

/// `RuleBasedBlockStateProvider.getState(level, random, pos)`.
pub fn provide_rule_based<W: FeatureWorld + ?Sized>(
    world: &mut W,
    provider: &RuleBasedStateProvider,
    rng: &mut FeatureRandom,
    pos: BlockPos,
) -> JavaState {
    for (pred, then) in provider.rules {
        if test_predicate(world, pred, pos) {
            return provide_state(then, rng, pos);
        }
    }
    provide_state(&provider.fallback, rng, pos)
}

#[inline]
fn noise_value(noise: super::noise::NoiseRef, pos: BlockPos, scale: f32) -> f64 {
    let s = scale as f64;
    noise.get_value(pos.x as f64 * s, pos.y as f64 * s, pos.z as f64 * s)
}

/// `NoiseBasedStateProvider.getRandomState(list, noiseValue)`.
#[inline]
fn pick_by_noise(states: &[JavaState], value: f64) -> JavaState {
    let d = ((1.0 + value) / 2.0).clamp(0.0, 0.9999);
    states[(d * states.len() as f64) as usize]
}

/// `Mth.clampedMap`.
#[inline]
pub fn clamped_map(v: f64, from_lo: f64, from_hi: f64, to_lo: f64, to_hi: f64) -> f64 {
    let t = ((v - from_lo) / (from_hi - from_lo)).clamp(0.0, 1.0);
    to_lo + t * (to_hi - to_lo)
}

/// Whether `dir`'s face of the block at `pos` is sturdy.
#[inline]
pub fn face_sturdy<W: FeatureWorld + ?Sized>(world: &mut W, pos: BlockPos, dir: Direction) -> bool {
    world.get_state(pos).is_face_sturdy(dir)
}
