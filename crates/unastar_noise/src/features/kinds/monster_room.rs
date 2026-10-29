//! `minecraft:monster_room`: the cobblestone dungeon with a spawner and up to
//! two chests.

use crate::features::blocks::{BlockPos, Direction, JavaState};
use crate::features::eval::{BlockEntityData, FeatureWorld};
use crate::features::java_blocks::{blocks as b, states, tags};
use crate::features::random::FeatureRandom;
use crate::features::survival::facing_of;

/// Spawner mobs Java picks from with `Util.getRandom` (zombie twice).
const MOBS: [&str; 4] = [
    "minecraft:skeleton",
    "minecraft:zombie",
    "minecraft:zombie",
    "minecraft:spider",
];

/// Java `minecraft:monster_room` (`MonsterRoomFeature`).
///
/// Room half-sizes are `nextInt(2) + 2` on x then z, height 4. The whole box
/// (walls included) must have solid floor and ceiling layers, and the walls
/// must have between 1 and 5 openings (air at wall height 0 and 1). The
/// interior becomes cave air (chests and spawners kept); wall blocks over a
/// non-solid block become cave air; other solid walls become cobblestone, the
/// floor mossy cobblestone with chance 3/4. Two chest passes each make three
/// attempts at a random interior floor position that is air with exactly one
/// solid horizontal neighbour. Finally a spawner goes at the origin. Every
/// replacement except the overhang clearing skips `features_cannot_replace`.
///
/// Block entities are set by the runtime: chest loot (`chests/simple_dungeon`)
/// and the spawner entity. The RNG draws Java makes for them are reproduced
/// here: one `nextLong` loot seed per placed chest, and the mob choice
/// `nextInt(4)` over skeleton, zombie, zombie, spider when the spawner exists.
pub fn place<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    origin: BlockPos,
) -> bool {
    let replaceable = |s: JavaState| !s.is_in(tags::FEATURES_CANNOT_REPLACE);
    let rx = rng.next_int_bounded(2) + 2;
    let (x0, x1) = (-rx - 1, rx + 1);
    let rz = rng.next_int_bounded(2) + 2;
    let (z0, z1) = (-rz - 1, rz + 1);

    let mut openings = 0;
    for x in x0..=x1 {
        for y in -1..=4 {
            for z in z0..=z1 {
                let p = origin.offset(x, y, z);
                let solid = world.get_state(p).is_solid();
                if (y == -1 || y == 4) && !solid {
                    return false;
                }
                if (x == x0 || x == x1 || z == z0 || z == z1)
                    && y == 0
                    && world.is_air(p)
                    && world.is_air(p.above())
                {
                    openings += 1;
                }
            }
        }
    }
    if !(1..=5).contains(&openings) {
        return false;
    }

    for x in x0..=x1 {
        for y in (-1..=3).rev() {
            for z in z0..=z1 {
                let p = origin.offset(x, y, z);
                let s = world.get_state(p);
                if x != x0 && y != -1 && z != z0 && x != x1 && z != z1 {
                    if !s.is(b::CHEST) && !s.is(b::SPAWNER) && replaceable(s) {
                        world.set_state(p, states::CAVE_AIR);
                    }
                } else if p.y >= world.min_y() && !world.get_state(p.below()).is_solid() {
                    world.set_state(p, states::CAVE_AIR);
                } else if s.is_solid() && !s.is(b::CHEST) {
                    let block = if y == -1 && rng.next_int_bounded(4) != 0 {
                        states::MOSSY_COBBLESTONE
                    } else {
                        states::COBBLESTONE
                    };
                    if replaceable(s) {
                        world.set_state(p, block);
                    }
                }
            }
        }
    }

    for _ in 0..2 {
        for _ in 0..3 {
            let x = origin.x + rng.next_int_bounded(rx * 2 + 1) - rx;
            let z = origin.z + rng.next_int_bounded(rz * 2 + 1) - rz;
            let p = BlockPos::new(x, origin.y, z);
            if !world.is_air(p) {
                continue;
            }
            let mut walls = 0;
            for dir in Direction::HORIZONTAL {
                if world.get_state(p.rel(dir)).is_solid() {
                    walls += 1;
                }
            }
            if walls == 1 {
                let chest = reorient(world, p, states::CHEST);
                if replaceable(world.get_state(p)) {
                    world.set_state(p, chest);
                }
                // RandomizableContainer.setBlockEntityLootTable: the seed is
                // drawn only when a chest block entity exists there.
                if world.is_block(p, b::CHEST) {
                    let seed = rng.next_long();
                    world.set_block_entity(
                        p,
                        BlockEntityData::loot("minecraft:chests/simple_dungeon", seed),
                    );
                }
                break;
            }
        }
    }

    if replaceable(world.get_state(origin)) {
        world.set_state(origin, states::SPAWNER);
    }
    if world.is_block(origin, b::SPAWNER) {
        let mob = MOBS[rng.next_int_bounded(MOBS.len() as i32) as usize];
        world.set_block_entity(origin, BlockEntityData::spawner(mob));
    }
    true
}

/// `StructurePiece.reorient` for a chest: face away from the single opaque
/// neighbour, else rotate the default facing off opaque blocks.
fn reorient<W: FeatureWorld + ?Sized>(world: &mut W, pos: BlockPos, state: JavaState) -> JavaState {
    let mut single: Option<Direction> = None;
    for dir in Direction::HORIZONTAL {
        let s = world.get_state(pos.rel(dir));
        if s.is(b::CHEST) {
            return state;
        }
        if s.is_solid_render() {
            if single.is_some() {
                single = None;
                break;
            }
            single = Some(dir);
        }
    }
    if let Some(dir) = single {
        return state.with("facing", dir.opposite().name());
    }
    let mut dir = facing_of(state).unwrap_or(Direction::North);
    if world.get_state(pos.rel(dir)).is_solid_render() {
        dir = dir.opposite();
    }
    if world.get_state(pos.rel(dir)).is_solid_render() {
        dir = dir.clockwise();
    }
    if world.get_state(pos.rel(dir)).is_solid_render() {
        dir = dir.opposite();
    }
    state.with("facing", dir.name())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::test_world::TestWorld;

    /// Solid stone up to y=100 with a 1x2 tunnel along x at z=0, y=20..=21.
    fn tunnel_world() -> TestWorld {
        let mut w = TestWorld::flat(100);
        for x in -12..=12 {
            w.blocks.insert(BlockPos::new(x, 20, 0), states::AIR);
            w.blocks.insert(BlockPos::new(x, 21, 0), states::AIR);
        }
        w
    }

    #[test]
    fn rejects_closed_rock() {
        let mut w = TestWorld::flat(100);
        let mut rng = FeatureRandom::new(4);
        assert!(!place(&mut w, &mut rng, BlockPos::new(0, 20, 0)));
        assert!(w.blocks.is_empty());
    }

    #[test]
    fn rejects_open_air() {
        let mut w = TestWorld::flat(0);
        let mut rng = FeatureRandom::new(4);
        assert!(!place(&mut w, &mut rng, BlockPos::new(0, 20, 0)));
    }

    #[test]
    fn builds_dungeon() {
        for seed in 0..20 {
            let mut w = tunnel_world();
            let origin = BlockPos::new(0, 20, 0);
            let mut rng = FeatureRandom::new(seed);
            assert!(place(&mut w, &mut rng, origin), "seed {seed}");
            assert_eq!(w.get_state(origin), states::SPAWNER);
            assert!(w.count(states::COBBLESTONE) + w.count(states::MOSSY_COBBLESTONE) > 0);
            let chests = w.blocks.values().filter(|s| s.is(b::CHEST)).count();
            assert!(chests <= 2);
            for (p, s) in &w.blocks {
                if s.is(b::CHEST) {
                    assert_eq!(p.y, 20);
                    assert!(p.x.abs() <= 3 && p.z.abs() <= 3, "{p:?}");
                }
                if *s == states::MOSSY_COBBLESTONE {
                    assert_eq!(p.y, 19, "mossy only on the floor: {p:?}");
                }
                assert!(p.x.abs() <= 12 && (19..=23).contains(&p.y) && p.z.abs() <= 4);
            }
        }
    }

    #[test]
    fn deterministic() {
        let run = |seed| {
            let mut w = tunnel_world();
            let mut rng = FeatureRandom::new(seed);
            place(&mut w, &mut rng, BlockPos::new(0, 20, 0));
            let mut v: Vec<_> = w.blocks.into_iter().collect();
            v.sort_by_key(|(p, _)| (p.x, p.y, p.z));
            (v, rng.next_int())
        };
        assert_eq!(run(3), run(3));
    }
}
