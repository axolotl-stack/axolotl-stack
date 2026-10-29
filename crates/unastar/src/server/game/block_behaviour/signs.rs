//! Signs: standing, wall and hanging signs.
//!
//! - **Standing and wall signs** come from one item (`<wood>_sign` places
//!   the wood's standing sign, see [`super::item_block`]). Placed on a top
//!   face the standing sign turns to face the player: `ground_sign_direction`
//!   = ⌊(yaw + 180) · 16 / 360 + ½⌋ mod 16 (0 faces south). Placed on a side
//!   face it becomes the wood's wall sign (`<wood>_wall_sign`; oak's is
//!   `wall_sign`) with `facing_direction` = the clicked face. A click on a
//!   bottom face places nothing. A standing sign needs a block with
//!   collision below, a wall sign one behind it (provisional: Minecraft
//!   Wiki).
//! - **Hanging signs** placed under a block hang from it (`hanging` 1).
//!   When the block's bottom face is full, the sign hangs from two chains
//!   and faces the player in one of four directions (`facing_direction`);
//!   otherwise (a fence, a chain, a wall) it hangs from one point
//!   (`attached_bit` 1) and faces the player in sixteen directions like a
//!   standing sign. Placed on a side face it is mounted on the wall
//!   (`hanging` 0) with its board sticking out of it: `facing_direction` is
//!   the horizontal direction across the clicked face that faces the player.
//!   A click on a top face places nothing. A hanging sign needs centre
//!   support from the block above; a wall-mounted one from a block at
//!   either end of its board (or a hanging sign there) (provisional:
//!   Minecraft Wiki / Java Edition).
//!
//! The sign editor does not open on placement and sign text is not kept:
//! signs have no block entity on the server yet.

use glam::{DVec3, IVec3};

use super::{Place, SupportKind, bit, int, provides_support, with};
use crate::entity::systems::BlockSource;
use crate::registry::BlockRegistry;
use crate::registry::block::{BlockEntry, StateValue};
use crate::registry::collision::block_collision_aabbs;
use crate::server::game::placement::Horizontal;

/// Sign state properties.
pub const GROUND_DIRECTION: &str = "ground_sign_direction";
pub const FACING: &str = "facing_direction";
pub const HANGING: &str = "hanging";
pub const ATTACHED: &str = "attached_bit";

/// `ground_sign_direction` of a sign facing a player with `yaw`.
pub fn ground_direction(yaw: f32) -> i32 {
    ((f64::from(yaw) + 180.0) * 16.0 / 360.0 + 0.5).floor() as i32 & 15
}

/// Whether the block at `pos` has any collision (what a standing or wall
/// sign rests on).
fn has_collision(blocks: &impl BlockSource, pos: IVec3) -> bool {
    blocks
        .block_at(pos)
        .is_some_and(|state| block_collision_aabbs(state).next().is_some())
}

fn is_hanging_sign(blocks: &impl BlockSource, pos: IVec3) -> bool {
    blocks.block_at(pos).is_some_and(|state| {
        blocks
            .registry()
            .get_by_runtime_id(state)
            .is_some_and(|entry| entry.state_value(state, HANGING).is_some())
    })
}

/// The standing and wall sign blocks of the wood of `block` (either one).
fn standing_and_wall<'a>(
    registry: &'a BlockRegistry,
    block: &BlockEntry,
) -> Option<(&'a BlockEntry, &'a BlockEntry)> {
    let id = block.string_id.as_str();
    let (standing, wall) = if id.ends_with("standing_sign") {
        (id.to_string(), id.replace("standing_sign", "wall_sign"))
    } else {
        (id.replace("wall_sign", "standing_sign"), id.to_string())
    };
    Some((
        registry.get_by_name(&standing)?,
        registry.get_by_name(&wall)?,
    ))
}

/// The direction a wall-mounted hanging sign's board runs towards from its
/// `facing_direction`: the board spans this direction and its opposite.
fn board_ends(facing: Horizontal) -> [Horizontal; 2] {
    [facing.clockwise(), facing.counter_clockwise()]
}

pub fn can_survive(blocks: &impl BlockSource, pos: IVec3, state: u32, entry: &BlockEntry) -> bool {
    if entry.state_value(state, HANGING).is_some() {
        if bit(entry, state, HANGING) {
            return provides_support(blocks, pos + IVec3::Y, IVec3::NEG_Y, SupportKind::Centre);
        }
        let Some(facing) = int(entry, state, FACING).and_then(Horizontal::from_facing) else {
            return false;
        };
        return board_ends(facing).iter().any(|end| {
            let offset = end.offset();
            provides_support(blocks, pos + offset, -offset, SupportKind::Centre)
                || is_hanging_sign(blocks, pos + offset)
        });
    }
    if entry.state_value(state, GROUND_DIRECTION).is_some() {
        return has_collision(blocks, pos - IVec3::Y);
    }
    int(entry, state, FACING)
        .and_then(Horizontal::from_facing)
        .is_some_and(|facing| has_collision(blocks, pos - facing.offset()))
}

pub fn placement(place: &Place<'_, impl BlockSource>) -> Option<u32> {
    if place.block.state_value(place.state, HANGING).is_some() {
        return hanging_placement(place);
    }
    let (standing, wall) = standing_and_wall(place.blocks.registry(), place.block)?;
    let face = place.input.face;
    if face == 1 {
        return Some(with(
            standing,
            standing.default_state_id,
            GROUND_DIRECTION,
            StateValue::Int(ground_direction(place.input.yaw)),
        ));
    }
    Horizontal::from_facing(i32::from(face))?;
    Some(with(
        wall,
        wall.default_state_id,
        FACING,
        StateValue::Int(i32::from(face)),
    ))
}

fn hanging_placement(place: &Place<'_, impl BlockSource>) -> Option<u32> {
    let block = place.block;
    let yaw = place.input.yaw;
    let looking = Horizontal::from_yaw(yaw);
    let state = block.default_state_id;
    match place.input.face {
        0 => {
            let attached = !provides_support(
                place.blocks,
                place.position + IVec3::Y,
                IVec3::NEG_Y,
                SupportKind::Full,
            );
            let state = with(block, state, HANGING, StateValue::Byte(1));
            let state = with(block, state, ATTACHED, StateValue::Byte(u8::from(attached)));
            Some(if attached {
                with(
                    block,
                    state,
                    GROUND_DIRECTION,
                    StateValue::Int(ground_direction(yaw)),
                )
            } else {
                with(
                    block,
                    state,
                    FACING,
                    StateValue::Int(looking.opposite().facing()),
                )
            })
        }
        face => {
            let clicked = Horizontal::from_facing(i32::from(face))?;
            // Across the clicked face, towards the player: against the
            // look direction's component along that axis.
            let across = clicked.clockwise();
            let (sin, cos) = f64::from(yaw).to_radians().sin_cos();
            let look = DVec3::new(-sin, 0.0, cos);
            let facing = if look.dot(across.offset().as_dvec3()) > 0.0 {
                across.opposite()
            } else {
                across
            };
            let state = with(block, state, HANGING, StateValue::Byte(0));
            Some(with(block, state, FACING, StateValue::Int(facing.facing())))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{FLOOR, Harness, state};
    use super::*;
    use crate::entity::systems::physics::tests::TestBlocks;
    use crate::server::game::placement::{PlacementInput, resolve_placement};

    const BLOCK: IVec3 = IVec3::new(0, FLOOR + 1, 0);

    fn entry(name: &str) -> &'static BlockEntry {
        BlockRegistry::vanilla().get_by_name(name).expect(name)
    }

    /// What clicking `face` of `clicked` with `held` places: block name and
    /// state, or `None` when refused.
    fn place(
        blocks: &TestBlocks,
        held: &str,
        clicked: IVec3,
        face: u8,
        yaw: f32,
    ) -> Option<(IVec3, &'static str, u32)> {
        let input = PlacementInput {
            clicked,
            face,
            click: DVec3::splat(0.5),
            yaw,
            pitch: 0.0,
        };
        let placement = resolve_placement(blocks, entry(held), &input)?;
        let block = BlockRegistry::vanilla().get_by_runtime_id(placement.state)?;
        Some((
            placement.position,
            block.string_id.as_str(),
            placement.state,
        ))
    }

    fn value(state: u32, property: &str) -> Option<StateValue> {
        BlockRegistry::vanilla()
            .get_by_runtime_id(state)?
            .state_value(state, property)
    }

    fn stone() -> TestBlocks {
        let mut blocks = TestBlocks::floor("minecraft:stone", FLOOR);
        blocks.set(BLOCK, "minecraft:stone");
        blocks
    }

    #[test]
    fn standing_signs_face_the_player_in_sixteen_steps() {
        for (yaw, direction) in [(180.0, 0), (0.0, 8), (90.0, 12), (270.0, 4), (202.5, 1)] {
            let (pos, name, placed) =
                place(&stone(), "minecraft:standing_sign", BLOCK, 1, yaw).unwrap();
            assert_eq!((pos, name), (BLOCK + IVec3::Y, "minecraft:standing_sign"));
            assert_eq!(
                value(placed, GROUND_DIRECTION),
                Some(StateValue::Int(direction)),
                "yaw {yaw}"
            );
        }
    }

    #[test]
    fn side_faces_take_the_wall_sign_of_the_same_wood() {
        let (pos, name, placed) =
            place(&stone(), "minecraft:standing_sign", BLOCK, 4, 0.0).unwrap();
        assert_eq!((pos, name), (BLOCK + IVec3::NEG_X, "minecraft:wall_sign"));
        assert_eq!(value(placed, FACING), Some(StateValue::Int(4)));
        let (_, name, placed) =
            place(&stone(), "minecraft:darkoak_standing_sign", BLOCK, 3, 0.0).unwrap();
        assert_eq!(name, "minecraft:darkoak_wall_sign");
        assert_eq!(value(placed, FACING), Some(StateValue::Int(3)));
        let (_, name, _) =
            place(&stone(), "minecraft:spruce_standing_sign", BLOCK, 5, 0.0).unwrap();
        assert_eq!(name, "minecraft:spruce_wall_sign");
    }

    #[test]
    fn signs_are_refused_under_blocks_and_without_a_block_below() {
        let mut floating = TestBlocks::default();
        floating.set(BLOCK, "minecraft:stone");
        assert_eq!(
            place(&floating, "minecraft:standing_sign", BLOCK, 0, 0.0),
            None
        );
        // Standing on a torch: nothing to rest on.
        let mut torch = stone();
        torch.set(BLOCK + IVec3::Y, "minecraft:torch");
        assert_eq!(
            place(&torch, "minecraft:standing_sign", BLOCK + IVec3::Y, 1, 0.0),
            None
        );
    }

    #[test]
    fn hanging_signs_hang_from_two_chains_under_a_full_face() {
        let mut blocks = TestBlocks::default();
        blocks.set(BLOCK, "minecraft:stone");
        // Looking east: the sign faces west, back at the player.
        let (pos, _, placed) =
            place(&blocks, "minecraft:oak_hanging_sign", BLOCK, 0, 270.0).unwrap();
        assert_eq!(pos, BLOCK - IVec3::Y);
        assert_eq!(value(placed, HANGING), Some(StateValue::Byte(1)));
        assert_eq!(value(placed, ATTACHED), Some(StateValue::Byte(0)));
        assert_eq!(value(placed, FACING), Some(StateValue::Int(4)));
    }

    #[test]
    fn hanging_signs_under_a_fence_hang_from_one_point() {
        let mut blocks = TestBlocks::default();
        blocks.set(BLOCK, "minecraft:oak_fence");
        let (_, _, placed) = place(&blocks, "minecraft:oak_hanging_sign", BLOCK, 0, 90.0).unwrap();
        assert_eq!(value(placed, HANGING), Some(StateValue::Byte(1)));
        assert_eq!(value(placed, ATTACHED), Some(StateValue::Byte(1)));
        assert_eq!(value(placed, GROUND_DIRECTION), Some(StateValue::Int(12)));
        assert_eq!(
            place(&stone(), "minecraft:oak_hanging_sign", BLOCK, 1, 0.0),
            None
        );
    }

    #[test]
    fn wall_hanging_signs_stick_out_of_the_wall_towards_the_player() {
        // Clicking the east face while looking north: the board runs east
        // from the wall and faces south, at the player.
        let (pos, _, placed) =
            place(&stone(), "minecraft:cherry_hanging_sign", BLOCK, 5, 180.0).unwrap();
        assert_eq!(pos, BLOCK + IVec3::X);
        assert_eq!(value(placed, HANGING), Some(StateValue::Byte(0)));
        assert_eq!(value(placed, FACING), Some(StateValue::Int(3)));
    }

    #[test]
    fn signs_break_with_their_drops_when_the_support_goes() {
        let mut h = Harness::new();
        h.put(BLOCK, state("minecraft:stone", &[]));
        let standing = BLOCK + IVec3::Y;
        h.set(
            standing,
            state(
                "minecraft:standing_sign",
                &[(GROUND_DIRECTION, StateValue::Int(4))],
            ),
        );
        let wall = BLOCK + IVec3::Z;
        h.set(
            wall,
            state("minecraft:wall_sign", &[(FACING, StateValue::Int(3))]),
        );
        let ceiling = BLOCK + IVec3::new(4, 3, 0);
        h.put(ceiling, state("minecraft:stone", &[]));
        let hanging = ceiling - IVec3::Y;
        h.set(
            hanging,
            state(
                "minecraft:oak_hanging_sign",
                &[(HANGING, StateValue::Byte(1))],
            ),
        );
        let air = state("minecraft:air", &[]);
        h.set(BLOCK, air);
        h.set(ceiling, air);
        h.tick(1);
        for pos in [standing, wall, hanging] {
            assert_eq!(h.name(pos), "minecraft:air", "{pos}");
        }
        assert_eq!(
            h.drops(),
            vec![
                ("minecraft:oak_hanging_sign".to_string(), 1),
                ("minecraft:oak_sign".to_string(), 1),
                ("minecraft:oak_sign".to_string(), 1),
            ]
        );
    }
}
