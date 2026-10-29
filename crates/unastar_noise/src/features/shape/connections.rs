//! Canonical Java 1.21.11 fence, pane, gate, stair and wall shape updates.
//!
//! Wall coverage is native collision geometry, not a full-cube approximation:
//! `wall_above_flags` stores coverage of TEST_SHAPE_POST (bit 0), followed by
//! TEST_SHAPES_WALL north/east/south/west (bits 1..=4), by the above state's
//! collision DOWN face. Native `isCovered(face, test)` tests that
//! `test ONLY_FIRST face` is empty.
//!
//! Waterlogged fences, panes, stairs and walls also schedule a water fluid tick
//! on every native update, including vertical identity updates. This module
//! computes states only; the generic feature tick API cannot express that
//! fluid-specific request and delay.

use crate::features::java_blocks::tags;
use crate::features::survival::facing_of;
use crate::features::{BlockPos, Direction, FeatureWorld, JavaState};

pub(super) fn update_shape<W: FeatureWorld + ?Sized>(
    world: &mut W,
    state: JavaState,
    pos: BlockPos,
    dir: Direction,
    neighbour: JavaState,
) -> Option<JavaState> {
    Some(match state.block().def().kind {
        "minecraft:fence" => {
            if dir.is_horizontal() {
                state.with_bool(dir.name(), fence_connects(state, neighbour, dir.opposite()))
            } else {
                state
            }
        }
        "minecraft:iron_bars" | "minecraft:stained_glass_pane" => {
            if dir.is_horizontal() {
                state.with_bool(dir.name(), pane_connects(neighbour, dir.opposite()))
            } else {
                state
            }
        }
        "minecraft:fence_gate" => {
            let facing = facing_of(state).expect("fence gate facing");
            if dir.axis() == facing.clockwise().axis() {
                state.with_bool(
                    "in_wall",
                    neighbour.is_in(tags::WALLS)
                        || world.get_state(pos.rel(dir.opposite())).is_in(tags::WALLS),
                )
            } else {
                state
            }
        }
        "minecraft:stair" | "minecraft:weathering_copper_stair" => {
            if dir.is_horizontal() {
                state.with("shape", stairs_shape(world, state, pos))
            } else {
                state
            }
        }
        "minecraft:wall" => update_wall(world, state, pos, dir, neighbour),
        _ => return None,
    })
}

fn gate_connects(state: JavaState, face: Direction) -> bool {
    state.block().def().kind == "minecraft:fence_gate"
        && facing_of(state).expect("fence gate facing").axis() == face.clockwise().axis()
}

fn fence_connects(fence: JavaState, neighbour: JavaState, face: Direction) -> bool {
    (!super::connection_exception(neighbour) && super::sturdy(neighbour, face))
        || (neighbour.is_in(tags::FENCES)
            && neighbour.is_in(tags::WOODEN_FENCES) == fence.is_in(tags::WOODEN_FENCES))
        || gate_connects(neighbour, face)
}

fn is_pane(state: JavaState) -> bool {
    matches!(
        state.block().def().kind,
        "minecraft:iron_bars" | "minecraft:stained_glass_pane"
    )
}

fn pane_connects(neighbour: JavaState, face: Direction) -> bool {
    (!super::connection_exception(neighbour) && super::sturdy(neighbour, face))
        || is_pane(neighbour)
        || neighbour.is_in(tags::WALLS)
}

fn is_stair(state: JavaState) -> bool {
    matches!(
        state.block().def().kind,
        "minecraft:stair" | "minecraft:weathering_copper_stair"
    )
}

fn can_take_stair_shape<W: FeatureWorld + ?Sized>(
    world: &mut W,
    state: JavaState,
    pos: BlockPos,
    dir: Direction,
) -> bool {
    let side = world.get_state(pos.rel(dir));
    !is_stair(side)
        || side.value("facing") != state.value("facing")
        || side.value("half") != state.value("half")
}

fn stairs_shape<W: FeatureWorld + ?Sized>(
    world: &mut W,
    state: JavaState,
    pos: BlockPos,
) -> &'static str {
    let facing = facing_of(state).expect("stair facing");
    let front = world.get_state(pos.rel(facing));
    if is_stair(front) && front.value("half") == state.value("half") {
        let other = facing_of(front).expect("stair facing");
        if other.axis() != facing.axis()
            && can_take_stair_shape(world, state, pos, other.opposite())
        {
            return if other == facing.counter_clockwise() {
                "outer_left"
            } else {
                "outer_right"
            };
        }
    }
    let back = world.get_state(pos.rel(facing.opposite()));
    if is_stair(back) && back.value("half") == state.value("half") {
        let other = facing_of(back).expect("stair facing");
        if other.axis() != facing.axis() && can_take_stair_shape(world, state, pos, other) {
            return if other == facing.counter_clockwise() {
                "inner_left"
            } else {
                "inner_right"
            };
        }
    }
    "straight"
}

fn wall_connects(neighbour: JavaState, face: Direction) -> bool {
    neighbour.is_in(tags::WALLS)
        || (!super::connection_exception(neighbour) && super::sturdy(neighbour, face))
        || is_pane(neighbour)
        || gate_connects(neighbour, face)
}

fn update_wall<W: FeatureWorld + ?Sized>(
    world: &mut W,
    state: JavaState,
    pos: BlockPos,
    dir: Direction,
    neighbour: JavaState,
) -> JavaState {
    if dir == Direction::Down {
        return state;
    }
    let above = if dir == Direction::Up {
        neighbour
    } else {
        world.get_state(pos.above())
    };
    let coverage = super::wall_above_flags(above);
    let mut updated = state;
    // Only the changed horizontal side reconnects. All other sides retain
    // their connection identity, but their low/tall geometry is recomputed.
    for (index, side) in Direction::HORIZONTAL.into_iter().enumerate() {
        let connected = if side == dir {
            wall_connects(neighbour, dir.opposite())
        } else {
            state.value(side.name()) != Some("none")
        };
        updated = updated.with(
            side.name(),
            if !connected {
                "none"
            } else if coverage & (1 << (index + 1)) != 0 {
                "tall"
            } else {
                "low"
            },
        );
    }
    updated.with_bool("up", should_raise_post(updated, above, coverage))
}

fn should_raise_post(state: JavaState, above: JavaState, coverage: u8) -> bool {
    if above.block().def().kind == "minecraft:wall" && above.value("up") == Some("true") {
        return true;
    }
    let north = state.value("north");
    let east = state.value("east");
    let south = state.value("south");
    let west = state.value("west");
    let north_none = north == Some("none");
    let east_none = east == Some("none");
    let south_none = south == Some("none");
    let west_none = west == Some("none");
    if (north_none && east_none && south_none && west_none)
        || north_none != south_none
        || east_none != west_none
    {
        return true;
    }
    if (north == Some("tall") && south == Some("tall"))
        || (east == Some("tall") && west == Some("tall"))
    {
        return false;
    }
    above.is_in(tags::WALL_POST_OVERRIDE) || coverage & 1 != 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::java_blocks::states;
    use crate::features::test_world::TestWorld;

    #[test]
    fn fences_require_matching_wood_tag_and_perpendicular_gate() {
        assert!(fence_connects(
            states::OAK_FENCE,
            states::BIRCH_FENCE,
            Direction::North
        ));
        assert!(!fence_connects(
            states::OAK_FENCE,
            states::NETHER_BRICK_FENCE,
            Direction::North
        ));
        let gate = states::OAK_FENCE_GATE.with("facing", "north");
        assert!(fence_connects(states::OAK_FENCE, gate, Direction::East));
        assert!(!fence_connects(states::OAK_FENCE, gate, Direction::North));
        assert!(pane_connects(
            states::WHITE_STAINED_GLASS_PANE,
            Direction::East
        ));
        assert!(pane_connects(states::COBBLESTONE_WALL, Direction::East));
    }

    #[test]
    fn gate_uses_opposite_wall_and_preserves_open_and_powered() {
        let mut world = TestWorld::default();
        let pos = BlockPos::new(0, 64, 0);
        let gate = states::OAK_FENCE_GATE
            .with("facing", "north")
            .with_bool("open", true)
            .with_bool("powered", true);
        world
            .blocks
            .insert(pos.rel(Direction::West), states::COBBLESTONE_WALL);
        let updated = update_shape(&mut world, gate, pos, Direction::East, states::AIR).unwrap();
        assert_eq!(updated, gate.with_bool("in_wall", true));
        assert_eq!(
            update_shape(
                &mut world,
                gate,
                pos,
                Direction::North,
                states::COBBLESTONE_WALL
            ),
            Some(gate)
        );
    }

    #[test]
    fn stair_outer_corner_half_and_blocker_rules() {
        let mut world = TestWorld::default();
        let pos = BlockPos::new(0, 64, 0);
        for half in ["bottom", "top"] {
            let stair = states::OAK_STAIRS
                .with("facing", "north")
                .with("half", half);
            let corner = states::STONE_STAIRS
                .with("facing", "west")
                .with("half", half);
            world.blocks.clear();
            world.blocks.insert(pos.rel(Direction::North), corner);
            assert_eq!(stairs_shape(&mut world, stair, pos), "outer_left");
            world.blocks.insert(pos.rel(Direction::East), stair);
            assert_eq!(stairs_shape(&mut world, stair, pos), "straight");
            world.blocks.remove(&pos.rel(Direction::East));
            world.blocks.insert(
                pos.rel(Direction::North),
                corner.with("half", if half == "top" { "bottom" } else { "top" }),
            );
            assert_eq!(stairs_shape(&mut world, stair, pos), "straight");
        }
    }

    #[test]
    fn stair_inner_corner_blocker_and_waterlogged_identity() {
        let mut world = TestWorld::default();
        let pos = BlockPos::new(0, 64, 0);
        let stair = states::OAK_STAIRS
            .with("facing", "north")
            .with_bool("waterlogged", true);
        world.blocks.insert(
            pos.rel(Direction::South),
            states::STONE_STAIRS.with("facing", "east"),
        );
        assert_eq!(
            update_shape(&mut world, stair, pos, Direction::West, states::AIR),
            Some(stair.with("shape", "inner_right"))
        );
        world.blocks.insert(pos.rel(Direction::East), stair);
        assert_eq!(stairs_shape(&mut world, stair, pos), "straight");
        let stale = stair.with("shape", "outer_left");
        assert_eq!(
            update_shape(&mut world, stale, pos, Direction::Up, states::STONE),
            Some(stale)
        );
    }

    #[test]
    fn wall_recomputes_sides_from_native_coverage_and_keeps_waterlogged() {
        let mut world = TestWorld::default();
        let pos = BlockPos::new(0, 64, 0);
        let wall = states::COBBLESTONE_WALL
            .with("north", "low")
            .with("south", "low")
            .with_bool("waterlogged", true);
        let updated = update_shape(&mut world, wall, pos, Direction::Up, states::STONE).unwrap();
        assert_eq!(
            updated,
            wall.with("north", "tall")
                .with("south", "tall")
                .with_bool("up", false)
        );
        assert_eq!(
            update_shape(&mut world, updated, pos, Direction::Down, states::AIR),
            Some(updated)
        );
        let updated = update_shape(&mut world, updated, pos, Direction::Up, states::AIR).unwrap();
        assert_eq!(updated, wall.with_bool("up", false));
    }

    #[test]
    fn wall_post_order_prioritizes_above_post_then_topology_then_tall_pairs() {
        let wall = states::COBBLESTONE_WALL
            .with("north", "low")
            .with("south", "low");
        assert!(!should_raise_post(wall, states::AIR, 0));
        assert!(should_raise_post(wall, states::AIR, 1));
        let tall = wall.with("north", "tall").with("south", "tall");
        assert!(!should_raise_post(tall, states::STONE, 1));
        assert!(should_raise_post(
            tall,
            states::COBBLESTONE_WALL.with_bool("up", true),
            0
        ));
        assert!(should_raise_post(wall.with("east", "low"), states::AIR, 0));
        assert!(should_raise_post(
            wall.with("north", "none").with("south", "none"),
            states::AIR,
            0
        ));
    }
}
