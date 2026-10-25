//! `minecraft:forest_rock`: a cluster of three small blobs (mossy cobblestone
//! boulders in vanilla).

use crate::features::blocks::{BlockPos, JavaState};
use crate::features::eval::FeatureWorld;
use crate::features::java_blocks::tags;
use crate::features::random::FeatureRandom;

/// Java `minecraft:forest_rock` (`BlockBlobFeature`).
///
/// Walks down from the origin until the block below is dirt-tagged or
/// `base_stone_overworld`; fails if that reaches `min_y + 3`. Then places three
/// blobs of `state`: each draws x/y/z half-extents in `0..=1`, fills the box
/// positions within `(sum * 0.333 + 0.5)` of the blob centre, and moves the
/// centre by `(-1 + nextInt(2), -nextInt(2), -1 + nextInt(2))`.
pub fn place<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    state: JavaState,
    origin: BlockPos,
) -> bool {
    let floor = world.min_y() + 3;
    let mut pos = origin;
    while pos.y > floor {
        let below = world.get_state(pos.below());
        if !below.is_air() && (below.is_in(tags::DIRT) || below.is_in(tags::BASE_STONE_OVERWORLD)) {
            break;
        }
        pos = pos.below();
    }
    if pos.y <= floor {
        return false;
    }
    for _ in 0..3 {
        let j = rng.next_int_bounded(2);
        let k = rng.next_int_bounded(2);
        let l = rng.next_int_bounded(2);
        let f = (j + k + l) as f32 * 0.333 + 0.5;
        let max = (f * f) as f64;
        for z in -l..=l {
            for y in -k..=k {
                for x in -j..=j {
                    let p = pos.offset(x, y, z);
                    if p.dist_sqr(pos) <= max {
                        world.set_state(p, state);
                    }
                }
            }
        }
        let dx = -1 + rng.next_int_bounded(2);
        let dy = -rng.next_int_bounded(2);
        let dz = -1 + rng.next_int_bounded(2);
        pos = pos.offset(dx, dy, dz);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::java_blocks::states;
    use crate::features::test_world::TestWorld;

    #[test]
    fn fails_without_ground() {
        let mut w = TestWorld::flat(-64);
        let mut rng = FeatureRandom::new(3);
        assert!(!place(
            &mut w,
            &mut rng,
            states::MOSSY_COBBLESTONE,
            BlockPos::new(0, 80, 0)
        ));
        assert!(w.blocks.is_empty());
    }

    #[test]
    fn rock_sits_on_ground_near_origin() {
        for seed in 0..30 {
            let mut w = TestWorld::flat(64);
            let mut rng = FeatureRandom::new(seed);
            assert!(place(
                &mut w,
                &mut rng,
                states::MOSSY_COBBLESTONE,
                BlockPos::new(0, 90, 0)
            ));
            let rocks: Vec<_> = w
                .blocks
                .iter()
                .filter(|(_, s)| **s == states::MOSSY_COBBLESTONE)
                .map(|(p, _)| *p)
                .collect();
            assert!(!rocks.is_empty());
            // The first blob centre is (0, 64, 0); each step moves at most one
            // block per axis (x/z in -1..=0, y in -1..=0) and blobs reach 1.
            for p in rocks {
                assert!((-4..=1).contains(&p.x) && (-4..=1).contains(&p.z), "{p:?}");
                assert!((61..=65).contains(&p.y), "{p:?}");
            }
        }
    }

    #[test]
    fn deterministic() {
        let run = |seed| {
            let mut w = TestWorld::flat(64);
            let mut rng = FeatureRandom::new(seed);
            place(
                &mut w,
                &mut rng,
                states::MOSSY_COBBLESTONE,
                BlockPos::new(3, 70, -2),
            );
            let mut v: Vec<_> = w.blocks.into_iter().collect();
            v.sort_by_key(|(p, _)| (p.x, p.y, p.z));
            v
        };
        assert_eq!(run(11), run(11));
    }
}
