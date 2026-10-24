//! `minecraft:geode`.
//!
//! Java shapes a geode from a few random distribution points around the
//! origin: every block of the `[min_gen_offset, max_gen_offset]` cube sums
//! `1 / sqrt(dist² + point_offset)` over the points (plus a small noise term)
//! and the sum picks the layer (filling, inner, middle, outer). An optional
//! crack is carved with three crack points on one side. Inner-layer blocks
//! may then grow one of the `inner_placements` (amethyst buds) on the first
//! free face.
//!
//! The geode noise is Java's `NormalNoise.create(new WorldgenRandom(new
//! LegacyRandomSource(levelSeed)), -4, 1.0)`; it only depends on the level seed,
//! so it is built once per seed and thread and reused.

use std::cell::RefCell;

use crate::features::blocks::{BlockPos, Direction, JavaState, NoiseSpec};
use crate::features::config::GeodeConfig;
use crate::features::eval::{FeatureWorld, provide_state};
use crate::features::java_blocks::{blocks as b, states};
use crate::features::noise::LegacyNoise;
use crate::features::random::FeatureRandom;

/// Noise parameters of the geode noise (`firstOctave -4`, amplitudes `[1.0]`).
const GEODE_NOISE: NoiseSpec = NoiseSpec {
    first_octave: -4,
    amplitudes: &[1.0],
};

thread_local! {
    /// The geode noise of the last level seed seen on this thread.
    static NOISE: RefCell<Option<(i64, LegacyNoise)>> = const { RefCell::new(None) };
}

/// Sample the geode noise for `seed` at a block position.
fn noise_at(seed: i64, pos: BlockPos) -> f64 {
    NOISE.with(|cell| {
        let mut cache = cell.borrow_mut();
        if !matches!(&*cache, Some((s, _)) if *s == seed) {
            *cache = Some((seed, LegacyNoise::new(seed, GEODE_NOISE)));
        }
        let (_, noise) = cache.as_ref().expect("geode noise just built");
        noise.get_value(pos.x as f64, pos.y as f64, pos.z as f64)
    })
}

/// `BuddingAmethystBlock.canClusterGrowAtState`: air, or a water block holding
/// a full (amount 8) fluid.
#[inline]
fn can_cluster_grow_at(state: JavaState) -> bool {
    state.is_air() || (state.is(b::WATER) && state.fluid().amount == 8)
}

/// `Feature.safeSetBlock` with `Feature.isReplaceable(cannot_replace)`.
#[inline]
fn safe_set<W: FeatureWorld + ?Sized>(
    world: &mut W,
    cfg: &GeodeConfig,
    pos: BlockPos,
    state: JavaState,
) {
    if !world.get_state(pos).is_in(cfg.cannot_replace) {
        world.set_state(pos, state);
    }
}

/// Java `GeodeFeature.place` (`minecraft:geode`).
///
/// Returns `false` (placing nothing) when more than `invalid_blocks_threshold`
/// distribution points land in air or in `invalid_blocks`. Blocks in
/// `cannot_replace` are never overwritten. Inner placements get their
/// `facing` set to the face they grow on and `waterlogged` set when that
/// position holds a fluid source.
pub fn place<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    cfg: &GeodeConfig,
    origin: BlockPos,
) -> bool {
    let seed = world.seed();
    let num_points = cfg.distribution_points.sample(rng);
    let distribution_scale = num_points as f64 / cfg.outer_wall_distance.max_value() as f64;
    let filling_threshold = 1.0 / cfg.filling.sqrt();
    let inner_threshold = 1.0 / (cfg.inner_layer + distribution_scale).sqrt();
    let middle_threshold = 1.0 / (cfg.middle_layer + distribution_scale).sqrt();
    let outer_threshold = 1.0 / (cfg.outer_layer + distribution_scale).sqrt();
    let crack_size = 1.0
        / (cfg.base_crack_size
            + rng.next_double() / 2.0
            + if num_points > 3 {
                distribution_scale
            } else {
                0.0
            })
        .sqrt();
    let generate_crack = (rng.next_float() as f64) < cfg.generate_crack_chance;

    let mut points: Vec<(BlockPos, i32)> = Vec::with_capacity(num_points.max(0) as usize);
    let mut invalid_points = 0;
    for _ in 0..num_points {
        let x = cfg.outer_wall_distance.sample(rng);
        let y = cfg.outer_wall_distance.sample(rng);
        let z = cfg.outer_wall_distance.sample(rng);
        let point = origin.offset(x, y, z);
        let state = world.get_state(point);
        if state.is_air() || state.is_in(cfg.invalid_blocks) {
            invalid_points += 1;
            if invalid_points > cfg.invalid_blocks_threshold {
                return false;
            }
        }
        points.push((point, cfg.point_offset.sample(rng)));
    }

    let mut crack_points = [BlockPos::default(); 3];
    let crack_count = if generate_crack {
        let side = rng.next_int_bounded(4);
        let o = num_points * 2 + 1;
        let (dx, dz) = match side {
            0 => (o, 0),
            1 => (0, o),
            2 => (o, o),
            _ => (0, 0),
        };
        crack_points = [
            origin.offset(dx, 7, dz),
            origin.offset(dx, 5, dz),
            origin.offset(dx, 1, dz),
        ];
        3
    } else {
        0
    };
    let crack_points = &crack_points[..crack_count];

    let mut potential_placements: Vec<BlockPos> = Vec::new();
    let (lo, hi) = (cfg.min_gen_offset, cfg.max_gen_offset);
    // `BlockPos.betweenClosed` order: x fastest, then y, then z.
    for dz in lo..=hi {
        for dy in lo..=hi {
            for dx in lo..=hi {
                let pos = origin.offset(dx, dy, dz);
                let noise_offset = noise_at(seed, pos) * cfg.noise_multiplier;
                let mut dist_sum = 0.0;
                for &(point, offset) in &points {
                    dist_sum += 1.0 / (pos.dist_sqr(point) + offset as f64).sqrt() + noise_offset;
                }
                let mut crack_sum = 0.0;
                for &crack in crack_points {
                    crack_sum += 1.0 / (pos.dist_sqr(crack) + cfg.crack_point_offset as f64).sqrt()
                        + noise_offset;
                }

                if dist_sum < outer_threshold {
                    continue;
                }
                if generate_crack && crack_sum >= crack_size && dist_sum < filling_threshold {
                    safe_set(world, cfg, pos, states::AIR);
                    for dir in Direction::ALL {
                        let neighbour = pos.rel(dir);
                        if !world.fluid(neighbour).is_empty() {
                            world.schedule_tick(neighbour);
                        }
                    }
                } else if dist_sum >= filling_threshold {
                    let s = provide_state(&cfg.filling_provider, rng, pos);
                    safe_set(world, cfg, pos, s);
                } else if dist_sum >= inner_threshold {
                    let use_alternate = (rng.next_float() as f64) < cfg.use_alternate_layer0_chance;
                    let provider = if use_alternate {
                        &cfg.alternate_inner_layer_provider
                    } else {
                        &cfg.inner_layer_provider
                    };
                    let s = provide_state(provider, rng, pos);
                    safe_set(world, cfg, pos, s);
                    if (!cfg.placements_require_layer0_alternate || use_alternate)
                        && (rng.next_float() as f64) < cfg.use_potential_placements_chance
                    {
                        potential_placements.push(pos);
                    }
                } else if dist_sum >= middle_threshold {
                    let s = provide_state(&cfg.middle_layer_provider, rng, pos);
                    safe_set(world, cfg, pos, s);
                } else {
                    let s = provide_state(&cfg.outer_layer_provider, rng, pos);
                    safe_set(world, cfg, pos, s);
                }
            }
        }
    }

    let placements = cfg.inner_placements;
    for crystal in potential_placements {
        let mut state = placements[rng.next_int_bounded(placements.len() as i32) as usize];
        for dir in Direction::ALL {
            if state.has("facing") {
                state = state.with("facing", dir.name());
            }
            let place_pos = crystal.rel(dir);
            let place_state = world.get_state(place_pos);
            if state.has("waterlogged") {
                state = state.with_bool("waterlogged", place_state.fluid().source);
            }
            if can_cluster_grow_at(place_state) {
                safe_set(world, cfg, place_pos, state);
                break;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::blocks::StateProvider;
    use crate::features::java_blocks::tags;
    use crate::features::test_world::TestWorld;
    use crate::features::values::IntProvider;

    static PLACEMENTS: [JavaState; 2] = [states::SMALL_AMETHYST_BUD, states::AMETHYST_CLUSTER];

    fn amethyst() -> GeodeConfig {
        GeodeConfig {
            filling_provider: StateProvider::Simple(states::AIR),
            inner_layer_provider: StateProvider::Simple(states::AMETHYST_BLOCK),
            alternate_inner_layer_provider: StateProvider::Simple(states::BUDDING_AMETHYST),
            middle_layer_provider: StateProvider::Simple(states::CALCITE),
            outer_layer_provider: StateProvider::Simple(states::SMOOTH_BASALT),
            inner_placements: &PLACEMENTS,
            cannot_replace: tags::FEATURES_CANNOT_REPLACE,
            invalid_blocks: tags::GEODE_INVALID_BLOCKS,
            filling: 1.7,
            inner_layer: 2.2,
            middle_layer: 3.2,
            outer_layer: 4.2,
            generate_crack_chance: 0.95,
            base_crack_size: 2.0,
            crack_point_offset: 2,
            use_potential_placements_chance: 0.35,
            use_alternate_layer0_chance: 0.083,
            placements_require_layer0_alternate: true,
            outer_wall_distance: IntProvider::Uniform { min: 4, max: 6 },
            distribution_points: IntProvider::Uniform { min: 3, max: 4 },
            point_offset: IntProvider::Uniform { min: 1, max: 2 },
            min_gen_offset: -16,
            max_gen_offset: 16,
            noise_multiplier: 0.05,
            invalid_blocks_threshold: 1,
        }
    }

    fn run(seed: i64) -> (TestWorld, bool) {
        let mut world = TestWorld::flat(100);
        world.seed = 1234;
        let mut rng = FeatureRandom::new(seed);
        let ok = place(&mut world, &mut rng, &amethyst(), BlockPos::new(0, 20, 0));
        (world, ok)
    }

    #[test]
    fn builds_layers_inside_stone() {
        let (world, ok) = run(7);
        assert!(ok);
        assert!(world.count(states::SMOOTH_BASALT) > 0);
        assert!(world.count(states::CALCITE) > 0);
        assert!(world.count(states::AMETHYST_BLOCK) > 0);
        // Everything written stays within the generation cube.
        for pos in world.blocks.keys() {
            assert!(
                (-16..=16).contains(&pos.x)
                    && (4..=36).contains(&pos.y)
                    && (-16..=16).contains(&pos.z)
            );
        }
    }

    #[test]
    fn buds_face_away_from_their_support() {
        for seed in 0..8 {
            let (mut world, _) = run(seed);
            let buds: Vec<(BlockPos, JavaState)> = world
                .blocks
                .iter()
                .filter(|(_, s)| s.is(b::SMALL_AMETHYST_BUD) || s.is(b::AMETHYST_CLUSTER))
                .map(|(p, s)| (*p, *s))
                .collect();
            for (pos, state) in buds {
                let facing = crate::features::survival::facing_of(state).expect("facing");
                let support = world.get_state(pos.rel(facing.opposite()));
                assert!(
                    support.is(b::BUDDING_AMETHYST),
                    "bud at {pos:?} not on budding amethyst"
                );
                assert_eq!(state.value("waterlogged"), Some("false"));
            }
        }
    }

    #[test]
    fn deterministic() {
        let (a, _) = run(99);
        let (b, _) = run(99);
        assert_eq!(a.blocks, b.blocks);
    }

    #[test]
    fn rejects_points_in_air() {
        let mut world = TestWorld::flat(-64);
        let mut rng = FeatureRandom::new(3);
        assert!(!place(
            &mut world,
            &mut rng,
            &amethyst(),
            BlockPos::new(0, 20, 0)
        ));
        assert!(world.blocks.is_empty());
    }

    #[test]
    fn never_replaces_cannot_replace() {
        let mut world = TestWorld::flat(100);
        world.fill = states::STONE;
        // A bedrock layer through the middle of the geode.
        for x in -16..=16 {
            for z in -16..=16 {
                world
                    .blocks
                    .insert(BlockPos::new(x, 20, z), states::BEDROCK);
            }
        }
        let mut rng = FeatureRandom::new(11);
        place(&mut world, &mut rng, &amethyst(), BlockPos::new(0, 20, 0));
        for x in -16..=16 {
            for z in -16..=16 {
                assert_eq!(world.blocks[&BlockPos::new(x, 20, z)], states::BEDROCK);
            }
        }
    }
}
