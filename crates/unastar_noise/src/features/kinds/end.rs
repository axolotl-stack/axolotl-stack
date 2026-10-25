//! End features: `minecraft:end_gateway`, `minecraft:end_island`,
//! `minecraft:end_platform`, `minecraft:end_spike`,
//! `minecraft:void_start_platform` and `minecraft:chorus_plant`.
//!
//! Block entities and entities are not created here: the end gateway's exit
//! (block-entity data) and the end crystals on top of the spikes (entities)
//! are set up by the runtime from the configs; these functions only write
//! block states and keep Java's random draws.

use crate::features::blocks::{BlockPos, Direction, JavaState};
use crate::features::config::{EndGatewayConfig, EndSpike, EndSpikeConfig};
use crate::features::eval::FeatureWorld;
use crate::features::java_blocks::{blocks as b, states, tags};
use crate::features::random::{FeatureRandom, LegacyRandom};

// ---- end_gateway ----------------------------------------------------------

/// Java `EndGatewayFeature.place` (`minecraft:end_gateway`).
///
/// Writes the gateway block in a 3x5x3 bedrock frame (air around it). The
/// gateway's exit position and `exact` flag from `cfg` are block-entity data
/// and are set by the runtime, not here.
pub fn place_end_gateway<W: FeatureWorld + ?Sized>(
    world: &mut W,
    _rng: &mut FeatureRandom,
    _cfg: &EndGatewayConfig,
    origin: BlockPos,
) -> bool {
    for dz in -1..=1 {
        for dy in -2..=2 {
            for dx in -1..=1 {
                let pos = origin.offset(dx, dy, dz);
                let same_x = dx == 0;
                let same_y = dy == 0;
                let same_z = dz == 0;
                let above_or_below = dy.abs() == 2;
                let state = if same_x && same_y && same_z {
                    states::END_GATEWAY
                } else if same_y {
                    states::AIR
                } else if (above_or_below && same_x && same_z)
                    || ((same_x || same_z) && !above_or_below)
                {
                    states::BEDROCK
                } else {
                    states::AIR
                };
                world.set_state(pos, state);
            }
        }
    }
    true
}

// ---- end_island -----------------------------------------------------------

/// Java `EndIslandFeature.place` (`minecraft:end_island`): stacked shrinking
/// end stone discs hanging down from the origin.
pub fn place_end_island<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    origin: BlockPos,
) -> bool {
    let mut size = rng.next_int_bounded(3) as f32 + 4.0;
    let mut y = 0;
    while size > 0.5 {
        let lo = (-size).floor() as i32;
        let hi = size.ceil() as i32;
        for x in lo..=hi {
            for z in lo..=hi {
                if ((x * x + z * z) as f32) <= (size + 1.0) * (size + 1.0) {
                    world.set_state(origin.offset(x, y, z), states::END_STONE);
                }
            }
        }
        size -= rng.next_int_bounded(2) as f32 + 0.5;
        y -= 1;
    }
    true
}

// ---- end_platform ---------------------------------------------------------

/// Java `EndPlatformFeature.place` (`minecraft:end_platform`): a 5x5
/// obsidian floor below the origin with 3 blocks of air above it. Blocks that
/// already match are left untouched.
pub fn place_end_platform<W: FeatureWorld + ?Sized>(
    world: &mut W,
    _rng: &mut FeatureRandom,
    origin: BlockPos,
) -> bool {
    for dx in -2..=2 {
        for dz in -2..=2 {
            for dy in -1..3 {
                let pos = origin.offset(dz, dy, dx);
                let state = if dy == -1 {
                    states::OBSIDIAN
                } else {
                    states::AIR
                };
                if !world.get_state(pos).is(state.block()) {
                    world.set_state(pos, state);
                }
            }
        }
    }
    true
}

// ---- void_start_platform --------------------------------------------------

/// Java `VoidStartPlatformFeature.place` (`minecraft:void_start_platform`).
///
/// Within one chunk of chunk (0, 0), fills the part of the 33x33 square
/// centred on (8, 8) that lies in the origin's chunk with stone at
/// `origin.y + 3`, with cobblestone at the centre.
pub fn place_void_start_platform<W: FeatureWorld + ?Sized>(
    world: &mut W,
    _rng: &mut FeatureRandom,
    origin: BlockPos,
) -> bool {
    let (cx, cz) = (origin.x >> 4, origin.z >> 4);
    // `PLATFORM_ORIGIN_CHUNK` is the chunk of (8, 3, 8): (0, 0).
    if cx.abs().max(cz.abs()) > 1 {
        return true;
    }
    let center = BlockPos::new(8, origin.y + 3, 8);
    for z in cz * 16..=cz * 16 + 15 {
        for x in cx * 16..=cx * 16 + 15 {
            if (center.x - x).abs().max((center.z - z).abs()) <= 16 {
                let pos = BlockPos::new(x, center.y, z);
                let state = if pos == center {
                    states::COBBLESTONE
                } else {
                    states::STONE
                };
                world.set_state(pos, state);
            }
        }
    }
    true
}

// ---- end_spike ------------------------------------------------------------

/// Number of vanilla end spikes.
pub const SPIKE_COUNT: usize = 10;

/// The ten end spikes Java derives from the level seed when the config lists
/// none (`EndSpikeFeature.getSpikesForLevel`): the key
/// `RandomSource.create(seed).nextLong() & 0xFFFF` seeds a Fisher-Yates
/// shuffle of the sizes `0..10`, and spike `i` sits on a radius-42 circle.
pub fn spikes_for_seed(seed: i64) -> [EndSpike; SPIKE_COUNT] {
    let key = LegacyRandom::new(seed).next_long() & 0xFFFF;
    let mut rng = LegacyRandom::new(key);
    let mut sizes: [i32; SPIKE_COUNT] = std::array::from_fn(|i| i as i32);
    // `Util.toShuffledList`.
    for i in (2..=SPIKE_COUNT).rev() {
        let swap_to = rng.next_int_bounded(i as i32) as usize;
        sizes.swap(i - 1, swap_to);
    }
    std::array::from_fn(|i| {
        let angle = 2.0 * (-std::f64::consts::PI + std::f64::consts::PI / 10.0 * i as f64);
        let size = sizes[i];
        EndSpike {
            center_x: (42.0 * angle.cos()).floor() as i32,
            center_z: (42.0 * angle.sin()).floor() as i32,
            radius: 2 + size / 3,
            height: 76 + size * 3,
            guarded: size == 1 || size == 2,
        }
    })
}

/// Java `EndSpikeFeature.place` (`minecraft:end_spike`).
///
/// Builds every spike whose centre lies in the origin's chunk: an obsidian
/// cylinder from the bottom of the world to `height` (clearing air above
/// y 65 up to `height + 10`), an iron-bar cage for guarded spikes, and the
/// bedrock and fire the end crystal sits on. When `cfg.spikes` is empty the
/// spikes come from [`spikes_for_seed`]. The end crystal itself is an entity
/// spawned by the runtime (with `crystal_invulnerable` and
/// `crystal_beam_target`); its random yaw is still drawn here.
pub fn place_end_spike<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    cfg: &EndSpikeConfig,
    origin: BlockPos,
) -> bool {
    let derived;
    let spikes: &[EndSpike] = if cfg.spikes.is_empty() {
        derived = spikes_for_seed(world.seed());
        &derived
    } else {
        cfg.spikes
    };
    for spike in spikes {
        if origin.x >> 4 == spike.center_x >> 4 && origin.z >> 4 == spike.center_z >> 4 {
            place_spike(world, rng, spike);
        }
    }
    true
}

fn place_spike<W: FeatureWorld + ?Sized>(world: &mut W, rng: &mut FeatureRandom, spike: &EndSpike) {
    let r = spike.radius;
    let limit = (r * r + 1) as f64;
    for z in spike.center_z - r..=spike.center_z + r {
        for y in world.min_y()..=spike.height + 10 {
            for x in spike.center_x - r..=spike.center_x + r {
                let pos = BlockPos::new(x, y, z);
                let dx = (x - spike.center_x) as f64;
                let dz = (z - spike.center_z) as f64;
                if dx * dx + dz * dz <= limit && y < spike.height {
                    world.set_state(pos, states::OBSIDIAN);
                } else if y > 65 {
                    world.set_state(pos, states::AIR);
                }
            }
        }
    }
    if spike.guarded {
        for dx in -2..=2 {
            for dz in -2..=2 {
                for dy in 0..=3 {
                    let x_edge = dx == -2 || dx == 2;
                    let z_edge = dz == -2 || dz == 2;
                    let top = dy == 3;
                    if !(x_edge || z_edge || top) {
                        continue;
                    }
                    let north_south = x_edge || top;
                    let east_west = z_edge || top;
                    let state = states::IRON_BARS
                        .with_bool("north", north_south && dz != -2)
                        .with_bool("south", north_south && dz != 2)
                        .with_bool("west", east_west && dx != -2)
                        .with_bool("east", east_west && dx != 2);
                    world.set_state(
                        BlockPos::new(spike.center_x + dx, spike.height + dy, spike.center_z + dz),
                        state,
                    );
                }
            }
        }
    }
    // End crystal yaw (`random.nextFloat() * 360`); the entity is spawned by the runtime.
    let _yaw = rng.next_float() * 360.0;
    let crystal = BlockPos::new(spike.center_x, spike.height + 1, spike.center_z);
    world.set_state(crystal.below(), states::BEDROCK);
    let fire = fire_state(world, crystal);
    world.set_state(crystal, fire);
}

/// `FireBlock.getState(level, pos)` for a fire placed on the just-written
/// bedrock: soul fire on soul-fire bases, otherwise plain fire. Java only
/// adds the flammable-neighbour face properties when the block below is
/// neither flammable nor sturdy, which bedrock always is, so the default
/// fire state is used.
fn fire_state<W: FeatureWorld + ?Sized>(world: &mut W, pos: BlockPos) -> JavaState {
    if world
        .get_state(pos.below())
        .is_in(tags::SOUL_FIRE_BASE_BLOCKS)
    {
        states::SOUL_FIRE
    } else {
        states::FIRE
    }
}

// ---- chorus_plant ---------------------------------------------------------

/// Java `ChorusPlantFeature.place` (`minecraft:chorus_plant`).
///
/// Needs an air origin on end stone; grows a full chorus tree with
/// `ChorusFlowerBlock.generatePlant(level, origin, random, 8)`.
pub fn place_chorus_plant<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    origin: BlockPos,
) -> bool {
    if !world.is_air(origin) || !world.get_state(origin.below()).is(b::END_STONE) {
        return false;
    }
    let state = chorus_connections(world, origin);
    world.set_state(origin, state);
    grow_chorus(world, rng, origin, origin, 8, 0);
    true
}

/// `ChorusPlantBlock.getStateWithConnections` for a chorus plant at `pos`.
fn chorus_connections<W: FeatureWorld + ?Sized>(world: &mut W, pos: BlockPos) -> JavaState {
    let connects = |s: JavaState| s.is(b::CHORUS_PLANT) || s.is(b::CHORUS_FLOWER);
    let down = world.get_state(pos.below());
    let up = world.get_state(pos.above());
    let north = world.get_state(pos.rel(Direction::North));
    let east = world.get_state(pos.rel(Direction::East));
    let south = world.get_state(pos.rel(Direction::South));
    let west = world.get_state(pos.rel(Direction::West));
    states::CHORUS_PLANT
        .with_bool("down", connects(down) || down.is(b::END_STONE))
        .with_bool("up", connects(up))
        .with_bool("north", connects(north))
        .with_bool("east", connects(east))
        .with_bool("south", connects(south))
        .with_bool("west", connects(west))
}

/// `ChorusFlowerBlock.allNeighborsEmpty`.
fn chorus_neighbours_empty<W: FeatureWorld + ?Sized>(
    world: &mut W,
    pos: BlockPos,
    ignore: Option<Direction>,
) -> bool {
    Direction::HORIZONTAL
        .into_iter()
        .all(|dir| Some(dir) == ignore || world.is_air(pos.rel(dir)))
}

/// `ChorusFlowerBlock.growTreeRecursive`.
fn grow_chorus<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    current: BlockPos,
    start: BlockPos,
    max_spread: i32,
    depth: i32,
) {
    let mut height = rng.next_int_bounded(4) + 1;
    if depth == 0 {
        height += 1;
    }
    for i in 0..height {
        let pos = current.offset(0, i + 1, 0);
        if !chorus_neighbours_empty(world, pos, None) {
            return;
        }
        let s = chorus_connections(world, pos);
        world.set_state(pos, s);
        let s = chorus_connections(world, pos.below());
        world.set_state(pos.below(), s);
    }
    let mut placed_stem = false;
    if depth < 4 {
        let mut stems = rng.next_int_bounded(4);
        if depth == 0 {
            stems += 1;
        }
        for _ in 0..stems {
            let dir = Direction::HORIZONTAL[rng.next_int_bounded(4) as usize];
            let target = current.offset(0, height, 0).rel(dir);
            if (target.x - start.x).abs() < max_spread
                && (target.z - start.z).abs() < max_spread
                && world.is_air(target)
                && world.is_air(target.below())
                && chorus_neighbours_empty(world, target, Some(dir.opposite()))
            {
                placed_stem = true;
                let s = chorus_connections(world, target);
                world.set_state(target, s);
                let back = target.rel(dir.opposite());
                let s = chorus_connections(world, back);
                world.set_state(back, s);
                grow_chorus(world, rng, target, start, max_spread, depth + 1);
            }
        }
    }
    if !placed_stem {
        world.set_state(
            current.offset(0, height, 0),
            states::CHORUS_FLOWER.with_int("age", 5),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::test_world::TestWorld;

    #[test]
    fn spike_positions_match_vanilla_circle() {
        let expected = [
            (42, 0),
            (33, 24),
            (12, 39),
            (-13, 39),
            (-34, 24),
            (-42, -1),
            (-34, -25),
            (-13, -40),
            (12, -40),
            (33, -25),
        ];
        let spikes = spikes_for_seed(12345);
        for (spike, (x, z)) in spikes.iter().zip(expected) {
            assert_eq!((spike.center_x, spike.center_z), (x, z));
        }
        // Sizes are a permutation of 0..10: heights 76..=103 step 3, two guarded.
        let mut heights: Vec<i32> = spikes.iter().map(|s| s.height).collect();
        heights.sort();
        assert_eq!(heights, (0..10).map(|s| 76 + s * 3).collect::<Vec<_>>());
        assert_eq!(spikes.iter().filter(|s| s.guarded).count(), 2);
    }

    #[test]
    fn spike_built_only_in_its_chunk() {
        let mut w = TestWorld::flat(-64);
        w.seed = 42;
        let cfg = EndSpikeConfig {
            crystal_invulnerable: false,
            spikes: &[],
            crystal_beam_target: None,
        };
        let mut rng = FeatureRandom::new(1);
        place_end_spike(&mut w, &mut rng, &cfg, BlockPos::new(100, 0, 100));
        assert!(w.blocks.is_empty());

        let spike = spikes_for_seed(42)[0];
        place_end_spike(
            &mut w,
            &mut rng,
            &cfg,
            BlockPos::new(spike.center_x, 0, spike.center_z),
        );
        let c = BlockPos::new(spike.center_x, 0, spike.center_z);
        assert_eq!(w.blocks[&c.with_y(spike.height - 1)], states::OBSIDIAN);
        assert_eq!(w.blocks[&c.with_y(spike.height)], states::BEDROCK);
        assert_eq!(w.blocks[&c.with_y(spike.height + 1)], states::FIRE);
    }

    #[test]
    fn gateway_frame() {
        let mut w = TestWorld::flat(-64);
        let mut rng = FeatureRandom::new(0);
        let cfg = EndGatewayConfig {
            exit: None,
            exact: false,
        };
        let o = BlockPos::new(0, 80, 0);
        place_end_gateway(&mut w, &mut rng, &cfg, o);
        assert_eq!(w.blocks[&o], states::END_GATEWAY);
        assert_eq!(w.blocks[&o.offset(0, 2, 0)], states::BEDROCK);
        assert_eq!(w.blocks[&o.offset(0, -2, 0)], states::BEDROCK);
        assert_eq!(w.blocks[&o.offset(1, 1, 0)], states::BEDROCK);
        assert_eq!(w.blocks[&o.offset(1, 1, 1)], states::AIR);
        assert_eq!(w.blocks[&o.offset(1, 0, 0)], states::AIR);
        assert_eq!(w.blocks.len(), 45);
    }

    #[test]
    fn island_and_platforms() {
        let mut w = TestWorld::flat(-64);
        let mut rng = FeatureRandom::new(9);
        place_end_island(&mut w, &mut rng, BlockPos::new(0, 60, 0));
        assert!(w.count(states::END_STONE) > 20);
        assert!(w.blocks.keys().all(|p| p.y <= 60));

        let mut w = TestWorld::flat(-64);
        place_end_platform(&mut w, &mut rng, BlockPos::new(100, 49, 0));
        assert_eq!(w.count(states::OBSIDIAN), 25);
        // Air was already there.
        assert_eq!(w.blocks.len(), 25);

        let mut w = TestWorld::flat(-64);
        place_void_start_platform(&mut w, &mut rng, BlockPos::new(0, 0, 0));
        assert_eq!(w.blocks[&BlockPos::new(8, 3, 8)], states::COBBLESTONE);
        assert_eq!(w.count(states::STONE), 255);
        let mut w = TestWorld::flat(-64);
        place_void_start_platform(&mut w, &mut rng, BlockPos::new(40, 0, 0));
        assert!(w.blocks.is_empty());
    }

    #[test]
    fn chorus_connections_are_consistent() {
        for seed in 0..10 {
            let mut w = TestWorld::flat(-64);
            for x in -10..=10 {
                for z in -10..=10 {
                    w.blocks.insert(BlockPos::new(x, 59, z), states::END_STONE);
                }
            }
            let mut rng = FeatureRandom::new(seed);
            assert!(place_chorus_plant(
                &mut w,
                &mut rng,
                BlockPos::new(0, 60, 0)
            ));
            assert!(w.blocks.values().any(|s| s.is(b::CHORUS_FLOWER)));
            let plants: Vec<(BlockPos, JavaState)> = w
                .blocks
                .iter()
                .filter(|(_, s)| s.is(b::CHORUS_PLANT))
                .map(|(p, s)| (*p, *s))
                .collect();
            for (pos, s) in plants {
                // Every connection points at chorus (or end stone below).
                for dir in Direction::ALL {
                    if s.value(dir.name()) == Some("true") {
                        let n = w.get_state(pos.rel(dir));
                        assert!(
                            n.is(b::CHORUS_PLANT) || n.is(b::CHORUS_FLOWER) || n.is(b::END_STONE),
                            "seed {seed}: {pos:?} connects {dir:?} to {n:?}"
                        );
                    }
                }
                assert!((pos.x).abs() < 8 && pos.z.abs() < 8);
            }
        }
        let mut w = TestWorld::flat(-64);
        let mut rng = FeatureRandom::new(0);
        assert!(!place_chorus_plant(
            &mut w,
            &mut rng,
            BlockPos::new(0, 60, 0)
        ));
    }
}
