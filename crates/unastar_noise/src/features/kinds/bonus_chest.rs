//! `minecraft:bonus_chest`: the spawn bonus chest with torches around it.
//!
//! Approximations (the block data has no collision or support shapes):
//! - Java accepts a surface position that is air or has an empty collision
//!   shape. Here a block counts as collision-free when it is not legacy-solid
//!   and no face is sturdy, except snow layers, which are collision-free only
//!   at `layers=1`. Thin non-full collision boxes (lily pads, sea pickles,
//!   candles) are therefore treated as collision-free.
//! - The torch survival check (`canSupportCenter` on the block below) uses
//!   the sturdy top face of the block below.

use crate::features::blocks::{BlockPos, Direction, Heightmap, JavaState};
use crate::features::eval::FeatureWorld;
use crate::features::java_blocks::{blocks as b, states};
use crate::features::random::FeatureRandom;

/// Java `minecraft:bonus_chest` (`BonusChestFeature`).
///
/// Shuffles the chunk's 16 block x coordinates, then its 16 z coordinates
/// (`Util.toShuffledList`), and walks them x-major. At the first column whose
/// `MOTION_BLOCKING_NO_LEAVES` surface position is air or collision-free it
/// places a chest, draws the loot-table seed (`nextLong`), puts a torch on each
/// horizontal neighbour (N, E, S, W) that can hold one, and succeeds. Fails if
/// no column qualifies.
///
/// The chest's loot table (`chests/spawn_bonus_chest`) and its seed are block
/// entity data set by the runtime; this only writes the block states.
pub fn place<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    origin: BlockPos,
) -> bool {
    let min_x = origin.x & !15;
    let min_z = origin.z & !15;
    let xs = shuffled(min_x, rng);
    let zs = shuffled(min_z, rng);
    for x in xs {
        for z in zs {
            let pos =
                world.heightmap_pos(Heightmap::MotionBlockingNoLeaves, BlockPos::new(x, 0, z));
            let s = world.get_state(pos);
            if !(s.is_air() || collision_empty(s)) {
                continue;
            }
            world.set_state(pos, states::CHEST);
            // RandomizableContainer.setBlockEntityLootTable draws the seed when
            // the chest block entity exists.
            if world.is_block(pos, b::CHEST) {
                rng.next_long();
            }
            for dir in Direction::HORIZONTAL {
                let p = pos.rel(dir);
                if world.get_state(p.below()).is_face_sturdy(Direction::Up) {
                    world.set_state(p, states::TORCH);
                }
            }
            return true;
        }
    }
    false
}

/// `Util.toShuffledList(IntStream.rangeClosed(min, min + 15), random)`.
fn shuffled(min: i32, rng: &mut FeatureRandom) -> [i32; 16] {
    let mut list: [i32; 16] = std::array::from_fn(|i| min + i as i32);
    for j in (2..=16).rev() {
        let k = rng.next_int_bounded(j as i32) as usize;
        list.swap(j - 1, k);
    }
    list
}

/// Approximate `getCollisionShape(...).isEmpty()` (see the module doc).
#[inline]
fn collision_empty(s: JavaState) -> bool {
    if s.is(b::SNOW) {
        return s.int_value("layers") == Some(1);
    }
    !s.is_solid() && !Direction::ALL.iter().any(|d| s.is_face_sturdy(*d))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::test_world::TestWorld;

    #[test]
    fn shuffle_is_permutation() {
        let mut rng = FeatureRandom::new(17);
        let mut v = shuffled(32, &mut rng);
        v.sort_unstable();
        assert_eq!(v, std::array::from_fn(|i| 32 + i as i32));
    }

    #[test]
    fn places_chest_and_torches() {
        let mut w = TestWorld::flat(64);
        let mut rng = FeatureRandom::new(5);
        assert!(place(&mut w, &mut rng, BlockPos::new(37, 0, -20)));
        let chests: Vec<_> = w
            .blocks
            .iter()
            .filter(|(_, s)| s.is(b::CHEST))
            .map(|(p, _)| *p)
            .collect();
        assert_eq!(chests.len(), 1);
        let c = chests[0];
        assert_eq!(c.y, 64);
        assert!(
            (32..48).contains(&c.x) && (-32..-16).contains(&c.z),
            "{c:?}"
        );
        assert_eq!(w.count(states::TORCH), 4);
        for dir in Direction::HORIZONTAL {
            assert_eq!(w.get_state(c.rel(dir)), states::TORCH);
        }
    }

    #[test]
    fn fails_when_surface_is_covered() {
        let mut w = TestWorld::flat(64);
        for x in 0..16 {
            for z in 0..16 {
                // Leaves are skipped by the heightmap but have a full collision.
                w.blocks.insert(BlockPos::new(x, 64, z), states::OAK_LEAVES);
            }
        }
        let mut rng = FeatureRandom::new(5);
        assert!(!place(&mut w, &mut rng, BlockPos::new(0, 0, 0)));
        assert_eq!(w.count(states::CHEST), 0);
    }

    #[test]
    fn deterministic() {
        let run = |seed| {
            let mut w = TestWorld::flat(64);
            let mut rng = FeatureRandom::new(seed);
            place(&mut w, &mut rng, BlockPos::new(0, 0, 0));
            let mut v: Vec<_> = w.blocks.into_iter().collect();
            v.sort_by_key(|(p, _)| (p.x, p.y, p.z));
            (v, rng.next_int())
        };
        assert_eq!(run(8), run(8));
    }
}
