//! Neighbour-derived states: fences, walls, panes, bars and stairs corners.
//!
//! - **Connections** (fences, walls, glass panes, iron bars) come from the
//!   connection rules of [`crate::registry::collision::connected_state`]:
//!   each side connects to a block of a joining kind or one whose facing
//!   face is full; a wall raises its post unless it runs straight
//!   (north–south or east–west only) with nothing above that forces a post
//!   (a torch, standing sign, standing lantern, a wall post, an open wooden
//!   trapdoor hinged over it). Walls therefore also change with the block
//!   above them.
//! - **Stairs corners** (`minecraft:corner`) follow the neighbouring stairs
//!   of the same half (provisional: Minecraft Wiki, not measured). With
//!   `facing` the side the raised half is on:
//!   - stairs in front (at `facing`) turned a quarter make an outer corner,
//!     `outer_left` when they face counter-clockwise of `facing`, unless
//!     the stairs beside on the side they turn away from face the same way
//!     and half as this one (a straight run continues);
//!   - otherwise stairs behind turned a quarter make an inner corner,
//!     `inner_left` when they face counter-clockwise of `facing`, with the
//!     same exception on the side they face;
//!   - otherwise the stairs stay straight (`none`).
//!
//! Placement computes the state from the neighbours. A neighbour update
//! recomputes fences, panes, bars and stairs at once and walls one game
//! tick later; a block is rewritten (through the change path,
//! which updates its own neighbours) only when the state differs.

use bevy_ecs::prelude::*;
use glam::IVec3;

use super::{Family, Place, bit, with};
use crate::entity::systems::BlockSource;
use crate::registry::block::{BlockEntry, StateValue};
use crate::server::game::placement::Horizontal;

/// Stairs state properties.
pub const CORNER: &str = "minecraft:corner";
pub const DIRECTION: &str = "weirdo_direction";
pub const UPSIDE_DOWN: &str = "upside_down_bit";

/// A neighbour of the connected block or stairs `state` at `pos` changed
/// (for walls: the tick scheduled after that): recompute its
/// neighbour-derived state.
pub fn neighbour_changed(world: &mut World, pos: IVec3, state: u32) {
    let family = super::family(world, state);
    let next = {
        let Some(entry) = world.registry().get_by_runtime_id(state) else {
            return;
        };
        shaped(&*world, entry, family, pos, state)
    };
    if next != state {
        crate::server::game::blocks::set_block(world, pos, next, false);
    }
}

/// Placement state of a connected block or stairs (see the module docs).
pub fn placement(place: &Place<'_, impl BlockSource>) -> Option<u32> {
    Some(shaped(
        place.blocks,
        place.block,
        Family::of(place.block),
        place.position,
        place.state,
    ))
}

/// `state` at `pos` with its neighbour-derived properties recomputed.
fn shaped(
    blocks: &impl BlockSource,
    entry: &BlockEntry,
    family: Family,
    pos: IVec3,
    state: u32,
) -> u32 {
    match family {
        Family::Connected => {
            crate::registry::collision::connected_state(blocks.registry(), pos, state, |p| {
                blocks.block_at(p)
            })
        }
        Family::Stairs => match stairs(entry, state) {
            Some(shape) => with(
                entry,
                state,
                CORNER,
                StateValue::Str(corner(blocks, pos, shape)),
            ),
            None => state,
        },
        _ => state,
    }
}

/// The side a stairs state's raised half is on, and whether it is upside
/// down. `None` for anything but stairs.
fn stairs(entry: &BlockEntry, state: u32) -> Option<(Horizontal, bool)> {
    let facing = match entry.state_value(state, DIRECTION)? {
        StateValue::Int(value) => Horizontal::from_weirdo(value)?,
        _ => return None,
    };
    entry.state_value(state, UPSIDE_DOWN)?;
    Some((facing, bit(entry, state, UPSIDE_DOWN)))
}

fn stairs_at(blocks: &impl BlockSource, pos: IVec3) -> Option<(Horizontal, bool)> {
    let state = blocks.block_at(pos)?;
    stairs(blocks.registry().get_by_runtime_id(state)?, state)
}

/// The `minecraft:corner` of stairs `shape` at `pos` (see the module docs).
fn corner(blocks: &impl BlockSource, pos: IVec3, shape: (Horizontal, bool)) -> &'static str {
    let (facing, upside_down) = shape;
    let turned = |other: Horizontal| other != facing && other != facing.opposite();
    // Stairs facing the same way and half on that side continue a straight
    // run, which keeps this block straight.
    let free = |side: Horizontal| stairs_at(blocks, pos + side.offset()) != Some(shape);
    if let Some((front, half)) = stairs_at(blocks, pos + facing.offset())
        && half == upside_down
        && turned(front)
        && free(front.opposite())
    {
        return if front == facing.counter_clockwise() {
            "outer_left"
        } else {
            "outer_right"
        };
    }
    if let Some((back, half)) = stairs_at(blocks, pos - facing.offset())
        && half == upside_down
        && turned(back)
        && free(back)
    {
        return if back == facing.counter_clockwise() {
            "inner_left"
        } else {
            "inner_right"
        };
    }
    "none"
}

#[cfg(test)]
mod tests {
    use glam::DVec3;

    use super::super::tests::{FLOOR, Harness, state};
    use super::*;
    use crate::entity::systems::physics::tests::TestBlocks;
    use crate::registry::BlockRegistry;
    use crate::server::game::placement::PlacementInput;

    const AT: IVec3 = IVec3::new(0, FLOOR + 1, 0);

    const INPUT: PlacementInput = PlacementInput {
        clicked: IVec3::ZERO,
        face: 1,
        click: DVec3::new(0.5, 1.0, 0.5),
        yaw: 0.0,
        pitch: 0.0,
    };

    /// What placing `state` at `position` puts there.
    fn placed(blocks: &impl BlockSource, state: u32, position: IVec3) -> u32 {
        let entry = BlockRegistry::vanilla().get_by_runtime_id(state).unwrap();
        super::super::placement_state(blocks, entry, state, &INPUT, position)
            .expect("placement accepted")
    }

    /// Place `name` at `pos` the way a player's click does.
    fn place(h: &mut Harness, pos: IVec3, name: &str) {
        let state = placed(&h.world, state(name, &[]), pos);
        h.set(pos, state);
    }

    #[test]
    fn fence_placed_next_to_a_fence_connects_both() {
        let mut h = Harness::new();
        place(&mut h, AT, "minecraft:oak_fence");
        place(&mut h, AT + IVec3::X, "minecraft:oak_fence");
        assert_eq!(
            h.value(AT, "minecraft:connection_east"),
            Some(StateValue::Byte(1))
        );
        assert_eq!(
            h.value(AT + IVec3::X, "minecraft:connection_west"),
            Some(StateValue::Byte(1))
        );
        assert_eq!(
            h.value(AT, "minecraft:connection_west"),
            Some(StateValue::Byte(0))
        );
        // Breaking one fence disconnects the other.
        h.set(AT + IVec3::X, state("minecraft:air", &[]));
        assert_eq!(
            h.value(AT, "minecraft:connection_east"),
            Some(StateValue::Byte(0))
        );
    }

    #[test]
    fn wall_drops_its_post_in_a_straight_run_unless_something_above_raises_it() {
        let mut h = Harness::new();
        let post = |h: &Harness, pos| h.value(pos, "wall_post_bit");
        place(&mut h, AT - IVec3::Z, "minecraft:cobblestone_wall");
        assert_eq!(post(&h, AT - IVec3::Z), Some(StateValue::Byte(1)));
        place(&mut h, AT + IVec3::Z, "minecraft:cobblestone_wall");
        place(&mut h, AT, "minecraft:cobblestone_wall");
        assert_eq!(post(&h, AT), Some(StateValue::Byte(0)));
        assert_eq!(
            h.value(AT, "wall_connection_type_north"),
            Some(StateValue::Str("short"))
        );
        assert_eq!(
            h.value(AT, "wall_connection_type_south"),
            Some(StateValue::Str("short"))
        );
        // The end walls connect one way only and keep their posts, one tick
        // after the neighbour update.
        assert_eq!(
            h.value(AT - IVec3::Z, "wall_connection_type_south"),
            Some(StateValue::Str("none"))
        );
        h.tick(1);
        assert_eq!(post(&h, AT - IVec3::Z), Some(StateValue::Byte(1)));
        assert_eq!(
            h.value(AT - IVec3::Z, "wall_connection_type_south"),
            Some(StateValue::Str("short"))
        );
        // A standing lantern on top raises the post; taking it away lowers it.
        h.set(
            AT + IVec3::Y,
            state("minecraft:lantern", &[("hanging", StateValue::Byte(0))]),
        );
        assert_eq!(post(&h, AT), Some(StateValue::Byte(0)));
        h.tick(1);
        assert_eq!(post(&h, AT), Some(StateValue::Byte(1)));
        h.set(AT + IVec3::Y, state("minecraft:air", &[]));
        h.tick(1);
        assert_eq!(post(&h, AT), Some(StateValue::Byte(0)));
    }

    fn stairs(facing: Horizontal, upside_down: u8) -> u32 {
        state(
            "minecraft:oak_stairs",
            &[
                (DIRECTION, StateValue::Int(facing.weirdo())),
                (UPSIDE_DOWN, StateValue::Byte(upside_down)),
            ],
        )
    }

    /// Bottom stairs facing north placed at the origin among `neighbours`.
    fn north_stairs_among(neighbours: &[(IVec3, u32)]) -> u32 {
        let mut blocks = TestBlocks::default();
        for (pos, state) in neighbours {
            blocks.set_state(*pos, *state);
        }
        placed(&blocks, stairs(Horizontal::North, 0), IVec3::ZERO)
    }

    fn corner_of(state: u32) -> Option<StateValue> {
        BlockRegistry::vanilla()
            .get_by_runtime_id(state)?
            .state_value(state, CORNER)
    }

    /// Which quarters of the upper half collide: north-west, north-east,
    /// south-west, south-east.
    fn raised(state: u32) -> [bool; 4] {
        [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)].map(|(x, z)| {
            let point = DVec3::new(x, 0.75, z);
            crate::registry::collision::block_collision_aabbs(state).any(|b| {
                (b.min.x..=b.max.x).contains(&point.x)
                    && (b.min.y..=b.max.y).contains(&point.y)
                    && (b.min.z..=b.max.z).contains(&point.z)
            })
        })
    }

    const BEHIND: IVec3 = IVec3::Z;
    const IN_FRONT: IVec3 = IVec3::NEG_Z;

    #[test]
    fn stairs_turned_behind_make_inner_corners() {
        let left = north_stairs_among(&[(BEHIND, stairs(Horizontal::West, 0))]);
        assert_eq!(corner_of(left), Some(StateValue::Str("inner_left")));
        // The raised half runs on along the stairs behind: north and west.
        assert_eq!(raised(left), [true, true, true, false]);
        let right = north_stairs_among(&[(BEHIND, stairs(Horizontal::East, 0))]);
        assert_eq!(corner_of(right), Some(StateValue::Str("inner_right")));
        assert_eq!(raised(right), [true, true, false, true]);
    }

    #[test]
    fn stairs_turned_in_front_make_outer_corners() {
        let left = north_stairs_among(&[(IN_FRONT, stairs(Horizontal::West, 0))]);
        assert_eq!(corner_of(left), Some(StateValue::Str("outer_left")));
        // Only the quarter under the raised half of the stairs in front.
        assert_eq!(raised(left), [true, false, false, false]);
        let right = north_stairs_among(&[(IN_FRONT, stairs(Horizontal::East, 0))]);
        assert_eq!(corner_of(right), Some(StateValue::Str("outer_right")));
        assert_eq!(raised(right), [false, true, false, false]);
    }

    #[test]
    fn stairs_stay_straight_across_halves_and_along_a_run() {
        let other_half = north_stairs_among(&[(BEHIND, stairs(Horizontal::West, 1))]);
        assert_eq!(corner_of(other_half), Some(StateValue::Str("none")));
        // Stairs facing the same way on the side the stairs behind face
        // continue a straight run.
        let run = north_stairs_among(&[
            (BEHIND, stairs(Horizontal::West, 0)),
            (IVec3::NEG_X, stairs(Horizontal::North, 0)),
        ]);
        assert_eq!(corner_of(run), Some(StateValue::Str("none")));
        let parallel = north_stairs_among(&[(BEHIND, stairs(Horizontal::South, 0))]);
        assert_eq!(corner_of(parallel), Some(StateValue::Str("none")));
    }

    #[test]
    fn stairs_corner_follows_a_neighbour_placed_later() {
        let mut h = Harness::new();
        h.set(AT, stairs(Horizontal::North, 0));
        h.set(AT + BEHIND, stairs(Horizontal::West, 0));
        assert_eq!(h.value(AT, CORNER), Some(StateValue::Str("inner_left")));
        h.set(AT + BEHIND, state("minecraft:air", &[]));
        assert_eq!(h.value(AT, CORNER), Some(StateValue::Str("none")));
    }
}
