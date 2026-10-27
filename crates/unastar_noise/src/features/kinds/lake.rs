//! `minecraft:lake` (the legacy carved-ellipsoid lake, used by the lava
//! lakes).
//!
//! Java also schedules a tick for `minecraft:air` at each carved air block
//! and marks the blocks above carved air and barrier blocks for chunk
//! post-processing. An air block tick does nothing, and post-processing is
//! chunk bookkeeping without block writes or random draws, so neither is
//! forwarded to the world (`FeatureWorld::schedule_tick` carries no block, and
//! a runtime would tick whatever later occupies the position).

use crate::features::blocks::{BlockPos, JavaState};
use crate::features::config::LakeConfig;
use crate::features::eval::{FeatureWorld, provide_state};
use crate::features::generated::BIOME_CLIMATE;
use crate::features::java_blocks::{blocks as b, states, tags};
use crate::features::random::FeatureRandom;

/// Cells of the 16 x 16 x 8 carve grid, indexed `(x * 16 + z) * 8 + y`.
const GRID: usize = 16 * 16 * 8;

#[inline]
fn cell(x: i32, z: i32, y: i32) -> usize {
    ((x * 16 + z) * 8 + y) as usize
}

/// A cell outside the carved volume with a carved face neighbour.
#[inline]
fn is_shell(grid: &[bool; GRID], x: i32, z: i32, y: i32) -> bool {
    !grid[cell(x, z, y)]
        && (x < 15 && grid[cell(x + 1, z, y)]
            || x > 0 && grid[cell(x - 1, z, y)]
            || z < 15 && grid[cell(x, z + 1, y)]
            || z > 0 && grid[cell(x, z - 1, y)]
            || y < 7 && grid[cell(x, z, y + 1)]
            || y > 0 && grid[cell(x, z, y - 1)])
}

/// `BlockState.liquid()`: the blocks Java flags as liquid (water, lava and
/// bubble columns).
#[inline]
fn is_liquid_block(state: JavaState) -> bool {
    state.is(b::WATER) || state.is(b::LAVA) || state.is(b::BUBBLE_COLUMN)
}

/// `LakeFeature.canReplaceBlock`.
#[inline]
fn can_replace(state: JavaState) -> bool {
    !state.is_in(tags::FEATURES_CANNOT_REPLACE)
}

/// Java `minecraft:lake` (`LakeFeature`).
///
/// Returns `false` when `origin.y <= min_y + 4`. Otherwise the grid origin is
/// moved 4 blocks down and `nextInt(4) + 4` ellipsoids (six `nextDouble`s
/// each) are carved into a 16x8x16 grid. The fluid state is drawn next; the
/// lake is rejected (`false`, nothing written) if a shell cell in the upper
/// half holds a liquid block, or a shell cell in the lower half is neither
/// solid nor the fluid state. Carved cells become fluid below grid y 4 and native
/// cave air from y 4 up, skipping `features_cannot_replace` blocks. The barrier state is
/// drawn last; unless it is air, solid shell blocks that are not
/// `lava_pool_stone_cannot_replace` become barrier (always below y 4, on
/// `nextInt(2) != 0` from y 4 up). Water lakes then freeze grid y 4 to ice
/// where the biome's `shouldFreeze` holds.
pub fn place<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    cfg: &LakeConfig,
    origin: BlockPos,
) -> bool {
    if origin.y <= world.min_y() + 4 {
        return false;
    }
    let origin = origin.offset(0, -4, 0);
    let mut grid = [false; GRID];
    let count = rng.next_int_bounded(4) + 4;
    for _ in 0..count {
        let sx = rng.next_double() * 6.0 + 3.0;
        let sy = rng.next_double() * 4.0 + 2.0;
        let sz = rng.next_double() * 6.0 + 3.0;
        let cx = rng.next_double() * (16.0 - sx - 2.0) + 1.0 + sx / 2.0;
        let cy = rng.next_double() * (8.0 - sy - 4.0) + 2.0 + sy / 2.0;
        let cz = rng.next_double() * (16.0 - sz - 2.0) + 1.0 + sz / 2.0;
        for x in 1..15 {
            for z in 1..15 {
                for y in 1..7 {
                    let dx = (x as f64 - cx) / (sx / 2.0);
                    let dy = (y as f64 - cy) / (sy / 2.0);
                    let dz = (z as f64 - cz) / (sz / 2.0);
                    if dx * dx + dy * dy + dz * dz < 1.0 {
                        grid[cell(x, z, y)] = true;
                    }
                }
            }
        }
    }

    let fluid = provide_state(&cfg.fluid, rng, origin);

    for x in 0..16 {
        for z in 0..16 {
            for y in 0..8 {
                if !is_shell(&grid, x, z, y) {
                    continue;
                }
                let state = world.get_state(origin.offset(x, y, z));
                if y >= 4 && is_liquid_block(state) {
                    return false;
                }
                if y < 4 && !state.is_solid() && state != fluid {
                    return false;
                }
            }
        }
    }

    for x in 0..16 {
        for z in 0..16 {
            for y in 0..8 {
                if !grid[cell(x, z, y)] {
                    continue;
                }
                let pos = origin.offset(x, y, z);
                if can_replace(world.get_state(pos)) {
                    world.set_state(pos, if y >= 4 { states::CAVE_AIR } else { fluid });
                }
            }
        }
    }

    let barrier = provide_state(&cfg.barrier, rng, origin);
    if !barrier.is_air() {
        for x in 0..16 {
            for z in 0..16 {
                for y in 0..8 {
                    if is_shell(&grid, x, z, y) && (y < 4 || rng.next_int_bounded(2) != 0) {
                        let pos = origin.offset(x, y, z);
                        let state = world.get_state(pos);
                        if state.is_solid() && !state.is_in(tags::LAVA_POOL_STONE_CANNOT_REPLACE) {
                            world.set_state(pos, barrier);
                        }
                    }
                }
            }
        }
    }

    if fluid.fluid().is_water() {
        for x in 0..16 {
            for z in 0..16 {
                let pos = origin.offset(x, 4, z);
                let climate = &BIOME_CLIMATE[world.biome(pos) as usize];
                if climate.should_freeze(world, pos, false) && can_replace(world.get_state(pos)) {
                    world.set_state(pos, states::ICE);
                }
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::blocks::StateProvider;
    use crate::features::generated::biome_index;
    use crate::features::test_world::TestWorld;

    const LAVA: LakeConfig = LakeConfig {
        fluid: StateProvider::Simple(states::LAVA),
        barrier: StateProvider::Simple(states::STONE),
    };

    fn run(world: &mut TestWorld, cfg: &LakeConfig, seed: i64, origin: BlockPos) -> bool {
        let mut rng = FeatureRandom::new(seed);
        place(world, &mut rng, cfg, origin)
    }

    #[test]
    fn too_low_bails_without_drawing() {
        let mut world = TestWorld::flat(64);
        let mut rng = FeatureRandom::new(1);
        assert!(!place(
            &mut world,
            &mut rng,
            &LAVA,
            BlockPos::new(0, -60, 0)
        ));
        assert!(world.blocks.is_empty());
        assert_eq!(rng.next_long(), FeatureRandom::new(1).next_long());
    }


    #[test]
    fn rejects_air_in_lower_shell() {
        // Everything is air: the lower shell is not solid and not lava.
        let mut world = TestWorld::flat(-64);
        assert!(!run(&mut world, &LAVA, 4, BlockPos::new(0, 20, 0)));
        assert!(world.blocks.is_empty());
    }

    #[test]
    fn rejects_liquid_in_upper_shell() {
        // Water from grid y 4 up: the upper shell touches liquid.
        let mut world = TestWorld::flat(64);
        for x in -1..17 {
            for z in -1..17 {
                for y in 20..25 {
                    world.blocks.insert(BlockPos::new(x, y, z), states::WATER);
                }
            }
        }
        let before = world.blocks.len();
        assert!(!run(&mut world, &LAVA, 4, BlockPos::new(0, 20, 0)));
        assert_eq!(world.blocks.len(), before);
    }

    #[test]
    fn water_lake_freezes_in_cold_biome() {
        let water = LakeConfig {
            fluid: StateProvider::Simple(states::WATER),
            barrier: StateProvider::Simple(states::AIR),
        };
        // Carved cells at grid y 4 become air, so the freeze pass only finds
        // water that was already there: a source at grid corner (0, 4, 0)
        // freezes in snowy plains and not in plains.
        for (biome, expect_ice) in [
            ("minecraft:snowy_plains", true),
            ("minecraft:plains", false),
        ] {
            let mut world = TestWorld::flat(70);
            world.biome = biome_index(biome).unwrap();
            let origin = BlockPos::new(0, 63, 0);
            // Grid corner x = z = 0 is never carved nor next to a carved cell.
            world.blocks.insert(BlockPos::new(0, 63, 0), states::WATER);
            assert!(run(&mut world, &water, 9, origin));
            let at = world.get_state(BlockPos::new(0, 63, 0));
            assert_eq!(at == states::ICE, expect_ice, "{biome}");
        }
    }
}
