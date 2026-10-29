//! Attached blocks: torches, ladders, buttons, levers and vines. Each hangs
//! on a face of a neighbouring block and breaks (with its drops, one tick
//! after the neighbour update that took its support) when that face can no
//! longer hold it.
//!
//! - **Torches** (every block with `torch_facing_direction`):
//!   `torch_facing_direction` names the side the supporting block is on
//!   (`west`: the torch hangs on the east face of the block at x − 1;
//!   `top`: it stands on the block below). A click on a side or top face
//!   uses that face when it gives centre support; otherwise (and always for
//!   a click on a bottom face) the first supporting face of the block at
//!   z + 1, x − 1, z − 1, x + 1 is used, and failing those the torch stands
//!   on the block below (refused when that cannot hold it either). Fence and
//!   wall tops hold a standing torch; a mob spawner holds a torch on any
//!   face. Torches are refused in water and lava; the underwater torch is
//!   the exception (provisional: Minecraft Wiki). Redstone torches follow
//!   the same rules (provisional: not measured separately), as BDS 1.26.51
//!   does for the other torches.
//! - **Ladders** go on side faces only (`facing_direction` = the clicked
//!   face, 2..=5) and need a full face behind them (provisional: Minecraft
//!   Wiki).
//! - **Buttons** (`facing_direction` = the clicked face: 0 under a block, 1
//!   on top, 2..=5 on sides) and **levers** need a full face on the block
//!   they are attached to. A lever on a side face takes that face's
//!   direction as `lever_direction`; on a top or bottom face its axis
//!   follows the player's look (`up_north_south`/`down_north_south` looking
//!   north or south, `up_east_west`/`down_east_west` looking east or west).
//! - **Vines** (`vine_direction_bits`: 1 south, 2 west, 4 north, 8 east,
//!   each naming a side the vine clings to) go on side faces only and take
//!   the bit of the side the clicked block is on, added to a vine already in
//!   the cell. A side holds while the block on that side has a full face
//!   towards the vine or the vine above clings to the same side; a vine
//!   with no side holding breaks, and [`vine_neighbour_changed`] drops the
//!   sides that stopped holding (provisional: Minecraft Wiki).

use bevy_ecs::prelude::*;
use glam::IVec3;

use super::{Place, SupportKind, int, name_at, provides_support, with};
use crate::entity::systems::BlockSource;
use crate::registry::block::{BlockEntry, StateValue};
use crate::server::game::placement::{Horizontal, face_offset};

/// Attached-block state properties.
pub const TORCH_FACING: &str = "torch_facing_direction";
pub const FACING: &str = "facing_direction";
pub const LEVER_DIRECTION: &str = "lever_direction";
pub const VINE_BITS: &str = "vine_direction_bits";

/// The torch's fallback supports in the order they are tried: the side the
/// support is on.
const TORCH_FALLBACK: [Horizontal; 4] = [
    Horizontal::South,
    Horizontal::West,
    Horizontal::North,
    Horizontal::East,
];

/// Whether a torch at `pos` can hang on the block on its `side` (`None`:
/// stand on the block below).
fn torch_support(blocks: &impl BlockSource, pos: IVec3, side: Option<Horizontal>) -> bool {
    let offset = side.map_or(IVec3::NEG_Y, Horizontal::offset);
    let support = pos + offset;
    name_at(blocks, support) == Some("minecraft:mob_spawner")
        || provides_support(blocks, support, -offset, SupportKind::Centre)
}

/// The `torch_facing_direction` value of a torch supported from `side`.
fn torch_facing(side: Option<Horizontal>) -> &'static str {
    side.map_or("top", Horizontal::cardinal)
}

pub fn torch_can_survive(
    blocks: &impl BlockSource,
    pos: IVec3,
    state: u32,
    entry: &BlockEntry,
) -> bool {
    match entry.state_value(state, TORCH_FACING) {
        Some(StateValue::Str("top")) => torch_support(blocks, pos, None),
        Some(StateValue::Str(side)) => match Horizontal::from_cardinal(side) {
            Some(side) => torch_support(blocks, pos, Some(side)),
            // `unknown`: any face around it will do.
            None => {
                torch_support(blocks, pos, None)
                    || TORCH_FALLBACK
                        .iter()
                        .any(|side| torch_support(blocks, pos, Some(*side)))
            }
        },
        _ => true,
    }
}

pub fn torch_placement(place: &Place<'_, impl BlockSource>) -> Option<u32> {
    let blocks = place.blocks;
    let pos = place.position;
    let in_liquid = name_at(blocks, pos).is_some_and(|name| {
        matches!(
            name,
            "minecraft:water"
                | "minecraft:flowing_water"
                | "minecraft:lava"
                | "minecraft:flowing_lava"
        )
    });
    if in_liquid && place.block.string_id != "minecraft:underwater_torch" {
        return None;
    }
    let clicked = match place.input.face {
        1 => Some(None),
        face => Horizontal::from_facing(i32::from(face)).map(|dir| Some(dir.opposite())),
    };
    let side = clicked
        .filter(|side| torch_support(blocks, pos, *side))
        .or_else(|| {
            TORCH_FALLBACK
                .iter()
                .copied()
                .find(|side| torch_support(blocks, pos, Some(*side)))
                .map(Some)
        })
        .unwrap_or(None);
    Some(with(
        place.block,
        place.state,
        TORCH_FACING,
        StateValue::Str(torch_facing(side)),
    ))
}

/// The face of the block a `facing_direction` block is attached to: the
/// block at `pos - offset`, its face pointing along `offset`.
fn attached_face(facing: i32) -> Option<IVec3> {
    face_offset(u8::try_from(facing).ok()?)
}

pub fn ladder_can_survive(
    blocks: &impl BlockSource,
    pos: IVec3,
    state: u32,
    entry: &BlockEntry,
) -> bool {
    let Some(facing) = int(entry, state, FACING).filter(|facing| (2..=5).contains(facing)) else {
        return false;
    };
    attached_face(facing)
        .is_some_and(|face| provides_support(blocks, pos - face, face, SupportKind::Full))
}

pub fn ladder_placement(place: &Place<'_, impl BlockSource>) -> Option<u32> {
    let face = place.input.face;
    if !(2..=5).contains(&face) {
        return None;
    }
    Some(with(
        place.block,
        place.state,
        FACING,
        StateValue::Int(i32::from(face)),
    ))
}

/// The face ordinal a lever with `lever_direction` `value` was placed on.
fn lever_face(value: &str) -> Option<i32> {
    Some(match value {
        "down_east_west" | "down_north_south" => 0,
        "up_north_south" | "up_east_west" => 1,
        "north" => 2,
        "south" => 3,
        "west" => 4,
        "east" => 5,
        _ => return None,
    })
}

/// `lever_direction` of a lever placed on `face` by a player with `yaw`.
fn lever_direction(face: u8, yaw: f32) -> Option<&'static str> {
    let north_south = matches!(
        Horizontal::from_yaw(yaw),
        Horizontal::North | Horizontal::South
    );
    Some(match (face, north_south) {
        (0, true) => "down_north_south",
        (0, false) => "down_east_west",
        (1, true) => "up_north_south",
        (1, false) => "up_east_west",
        (2, _) => "north",
        (3, _) => "south",
        (4, _) => "west",
        (5, _) => "east",
        _ => return None,
    })
}

/// Buttons and levers.
pub fn switch_can_survive(
    blocks: &impl BlockSource,
    pos: IVec3,
    state: u32,
    entry: &BlockEntry,
) -> bool {
    let facing = match entry.state_value(state, LEVER_DIRECTION) {
        Some(StateValue::Str(value)) => lever_face(value),
        _ => int(entry, state, FACING),
    };
    facing
        .and_then(attached_face)
        .is_some_and(|face| provides_support(blocks, pos - face, face, SupportKind::Full))
}

/// Buttons and levers.
pub fn switch_placement(place: &Place<'_, impl BlockSource>) -> Option<u32> {
    let block = place.block;
    let face = place.input.face;
    if block.state_value(place.state, LEVER_DIRECTION).is_some() {
        let direction = lever_direction(face, place.input.yaw)?;
        let state = with(
            block,
            place.state,
            LEVER_DIRECTION,
            StateValue::Str(direction),
        );
        return Some(with(block, state, "open_bit", StateValue::Byte(0)));
    }
    if face > 5 {
        return None;
    }
    let state = with(block, place.state, FACING, StateValue::Int(i32::from(face)));
    Some(with(
        block,
        state,
        "button_pressed_bit",
        StateValue::Byte(0),
    ))
}

/// The `vine_direction_bits` bit of the side `side`.
fn vine_bit(side: Horizontal) -> i32 {
    match side {
        Horizontal::South => 1,
        Horizontal::West => 2,
        Horizontal::North => 4,
        Horizontal::East => 8,
    }
}

const VINE_SIDES: [Horizontal; 4] = [
    Horizontal::South,
    Horizontal::West,
    Horizontal::North,
    Horizontal::East,
];

/// Whether the side `side` of a vine at `pos` holds (see the module docs).
fn vine_side_holds(
    blocks: &impl BlockSource,
    pos: IVec3,
    side: Horizontal,
    entry: &BlockEntry,
) -> bool {
    let offset = side.offset();
    if provides_support(blocks, pos + offset, -offset, SupportKind::Full) {
        return true;
    }
    let above = pos + IVec3::Y;
    blocks.block_at(above).is_some_and(|other| {
        (entry.min_state_id..=entry.max_state_id).contains(&other)
            && int(entry, other, VINE_BITS).is_some_and(|bits| bits & vine_bit(side) != 0)
    })
}

/// The sides of the vine `state` at `pos` that still hold.
fn vine_held_bits(blocks: &impl BlockSource, pos: IVec3, state: u32, entry: &BlockEntry) -> i32 {
    let bits = int(entry, state, VINE_BITS).unwrap_or(0);
    VINE_SIDES
        .iter()
        .filter(|side| bits & vine_bit(**side) != 0 && vine_side_holds(blocks, pos, **side, entry))
        .fold(0, |held, side| held | vine_bit(*side))
}

pub fn vine_can_survive(
    blocks: &impl BlockSource,
    pos: IVec3,
    state: u32,
    entry: &BlockEntry,
) -> bool {
    vine_held_bits(blocks, pos, state, entry) != 0
}

pub fn vine_placement(place: &Place<'_, impl BlockSource>) -> Option<u32> {
    let clicked = Horizontal::from_facing(i32::from(place.input.face))?;
    let block = place.block;
    let existing = place
        .blocks
        .block_at(place.position)
        .filter(|state| (block.min_state_id..=block.max_state_id).contains(state))
        .and_then(|state| int(block, state, VINE_BITS))
        .unwrap_or(0);
    let bits = existing | vine_bit(clicked.opposite());
    Some(with(block, place.state, VINE_BITS, StateValue::Int(bits)))
}

/// A neighbour of the vine `state` at `pos` changed while at least one of
/// its sides still holds: drop the sides that no longer do.
pub fn vine_neighbour_changed(world: &mut World, pos: IVec3, state: u32) {
    let Some(entry) = world.registry().get_by_runtime_id(state) else {
        return;
    };
    let held = vine_held_bits(world, pos, state, entry);
    if held == 0 || Some(held) == int(entry, state, VINE_BITS) {
        return;
    }
    let next = with(entry, state, VINE_BITS, StateValue::Int(held));
    crate::server::game::blocks::set_block(world, pos, next, false);
}

#[cfg(test)]
mod tests {
    use glam::DVec3;

    use super::super::tests::{FLOOR, Harness, state};
    use super::*;
    use crate::entity::systems::physics::tests::TestBlocks;
    use crate::registry::BlockRegistry;
    use crate::server::game::placement::{PlacementInput, resolve_placement};

    fn entry(name: &str) -> &'static BlockEntry {
        BlockRegistry::vanilla().get_by_name(name).expect(name)
    }

    fn click(clicked: IVec3, face: u8, yaw: f32) -> PlacementInput {
        PlacementInput {
            clicked,
            face,
            click: DVec3::splat(0.5),
            yaw,
            pitch: 0.0,
        }
    }

    /// The name and `property` of what clicking `face` of `clicked` with
    /// `block` places, or `None` when refused.
    fn placed(
        blocks: &TestBlocks,
        block: &str,
        clicked: IVec3,
        face: u8,
        yaw: f32,
        property: &str,
    ) -> Option<(IVec3, StateValue)> {
        let placement = resolve_placement(blocks, entry(block), &click(clicked, face, yaw))?;
        let value = BlockRegistry::vanilla()
            .get_by_runtime_id(placement.state)?
            .state_value(placement.state, property)?;
        Some((placement.position, value))
    }

    const STONE_AT: IVec3 = IVec3::new(0, FLOOR + 1, 0);

    fn stone_on_floor() -> TestBlocks {
        let mut blocks = TestBlocks::floor("minecraft:stone", FLOOR);
        blocks.set(STONE_AT, "minecraft:stone");
        blocks
    }

    #[test]
    fn torches_name_the_side_their_support_is_on() {
        let blocks = stone_on_floor();
        for (face, cell, facing) in [
            (5, IVec3::X, "west"),
            (4, IVec3::NEG_X, "east"),
            (3, IVec3::Z, "north"),
            (2, IVec3::NEG_Z, "south"),
            (1, IVec3::Y, "top"),
        ] {
            assert_eq!(
                placed(
                    &blocks,
                    "minecraft:torch",
                    STONE_AT,
                    face,
                    0.0,
                    TORCH_FACING
                ),
                Some((STONE_AT + cell, StateValue::Str(facing))),
                "face {face}"
            );
        }
    }

    #[test]
    fn a_torch_clicked_on_a_face_that_cannot_hold_it_falls_back() {
        // The side of a bottom slab cannot hold a torch: it stands on the
        // floor in front of the slab instead.
        let mut blocks = TestBlocks::floor("minecraft:stone", FLOOR);
        blocks.set(STONE_AT, "minecraft:smooth_stone_slab");
        assert_eq!(
            placed(&blocks, "minecraft:torch", STONE_AT, 5, 0.0, TORCH_FACING),
            Some((STONE_AT + IVec3::X, StateValue::Str("top")))
        );
        // Under a block, with a wall at z + 1 of the cell: it hangs there.
        let mut blocks = TestBlocks::default();
        blocks.set(STONE_AT, "minecraft:stone");
        blocks.set(STONE_AT + IVec3::new(0, -1, 1), "minecraft:stone");
        assert_eq!(
            placed(
                &blocks,
                "minecraft:soul_torch",
                STONE_AT,
                0,
                0.0,
                TORCH_FACING
            ),
            Some((STONE_AT - IVec3::Y, StateValue::Str("south")))
        );
    }

    #[test]
    fn torches_are_refused_without_support_and_in_water() {
        let mut blocks = TestBlocks::default();
        blocks.set(STONE_AT, "minecraft:stone");
        assert_eq!(
            placed(&blocks, "minecraft:torch", STONE_AT, 0, 0.0, TORCH_FACING),
            None
        );
        let mut wet = stone_on_floor();
        wet.set(STONE_AT + IVec3::X, "minecraft:water");
        assert_eq!(
            placed(&wet, "minecraft:torch", STONE_AT, 5, 0.0, TORCH_FACING),
            None
        );
        assert_eq!(
            placed(
                &wet,
                "minecraft:underwater_torch",
                STONE_AT,
                5,
                0.0,
                TORCH_FACING
            ),
            Some((STONE_AT + IVec3::X, StateValue::Str("west")))
        );
    }

    #[test]
    fn torches_stand_on_fence_tops() {
        let mut blocks = TestBlocks::default();
        blocks.set(STONE_AT, "minecraft:oak_fence");
        assert_eq!(
            placed(&blocks, "minecraft:torch", STONE_AT, 1, 0.0, TORCH_FACING),
            Some((STONE_AT + IVec3::Y, StateValue::Str("top")))
        );
    }

    #[test]
    fn a_torch_breaks_with_its_drop_when_its_support_goes() {
        let mut h = Harness::new();
        h.put(STONE_AT, state("minecraft:stone", &[]));
        let torch = STONE_AT + IVec3::X;
        h.set(
            torch,
            state(
                "minecraft:torch",
                &[(TORCH_FACING, StateValue::Str("west"))],
            ),
        );
        h.set(STONE_AT, state("minecraft:air", &[]));
        h.tick(1);
        assert_eq!(h.name(torch), "minecraft:air");
        assert_eq!(h.drops(), vec![("minecraft:torch".to_string(), 1)]);
    }

    #[test]
    fn ladders_go_on_side_faces_only() {
        let blocks = stone_on_floor();
        assert_eq!(
            placed(&blocks, "minecraft:ladder", STONE_AT, 3, 0.0, FACING),
            Some((STONE_AT + IVec3::Z, StateValue::Int(3)))
        );
        assert_eq!(
            placed(&blocks, "minecraft:ladder", STONE_AT, 4, 0.0, FACING),
            Some((STONE_AT + IVec3::NEG_X, StateValue::Int(4)))
        );
        assert_eq!(
            placed(&blocks, "minecraft:ladder", STONE_AT, 1, 0.0, FACING),
            None
        );
        let mut slab = TestBlocks::floor("minecraft:stone", FLOOR);
        slab.set(STONE_AT, "minecraft:smooth_stone_slab");
        assert_eq!(
            placed(&slab, "minecraft:ladder", STONE_AT, 3, 0.0, FACING),
            None
        );
    }

    #[test]
    fn a_ladder_breaks_with_its_drop_when_the_block_behind_goes() {
        let mut h = Harness::new();
        h.put(STONE_AT, state("minecraft:stone", &[]));
        let ladder = STONE_AT + IVec3::NEG_Z;
        h.set(
            ladder,
            state("minecraft:ladder", &[(FACING, StateValue::Int(2))]),
        );
        h.set(STONE_AT, state("minecraft:air", &[]));
        h.tick(1);
        assert_eq!(h.name(ladder), "minecraft:air");
        assert_eq!(h.drops(), vec![("minecraft:ladder".to_string(), 1)]);
    }

    #[test]
    fn buttons_face_the_clicked_face_and_need_a_full_face() {
        let mut blocks = stone_on_floor();
        blocks.set(STONE_AT + IVec3::new(0, 2, 0), "minecraft:stone");
        for (face, cell) in [
            (1, IVec3::Y),
            (2, IVec3::NEG_Z),
            (3, IVec3::Z),
            (4, IVec3::NEG_X),
            (5, IVec3::X),
        ] {
            assert_eq!(
                placed(
                    &blocks,
                    "minecraft:stone_button",
                    STONE_AT,
                    face,
                    0.0,
                    FACING
                ),
                Some((STONE_AT + cell, StateValue::Int(i32::from(face)))),
                "face {face}"
            );
        }
        let ceiling = STONE_AT + IVec3::new(0, 2, 0);
        assert_eq!(
            placed(&blocks, "minecraft:wooden_button", ceiling, 0, 0.0, FACING),
            Some((STONE_AT + IVec3::Y, StateValue::Int(0)))
        );
        let mut fence = TestBlocks::default();
        fence.set(STONE_AT, "minecraft:oak_fence");
        assert_eq!(
            placed(&fence, "minecraft:stone_button", STONE_AT, 1, 0.0, FACING),
            None
        );
    }

    #[test]
    fn levers_take_the_face_direction_or_the_look_axis() {
        let mut blocks = stone_on_floor();
        let ceiling = STONE_AT + IVec3::new(0, 2, 0);
        blocks.set(ceiling, "minecraft:stone");
        for (face, yaw, direction) in [
            (2, 0.0, "north"),
            (3, 0.0, "south"),
            (4, 0.0, "west"),
            (5, 0.0, "east"),
            (1, 0.0, "up_north_south"),
            (1, 180.0, "up_north_south"),
            (1, 90.0, "up_east_west"),
            (1, 270.0, "up_east_west"),
        ] {
            assert_eq!(
                placed(
                    &blocks,
                    "minecraft:lever",
                    STONE_AT,
                    face,
                    yaw,
                    LEVER_DIRECTION
                )
                .map(|(_, value)| value),
                Some(StateValue::Str(direction)),
                "face {face} yaw {yaw}"
            );
        }
        for (yaw, direction) in [(180.0, "down_north_south"), (90.0, "down_east_west")] {
            assert_eq!(
                placed(&blocks, "minecraft:lever", ceiling, 0, yaw, LEVER_DIRECTION),
                Some((STONE_AT + IVec3::Y, StateValue::Str(direction)))
            );
        }
    }

    #[test]
    fn buttons_and_levers_break_with_their_drops_when_the_support_goes() {
        let mut h = Harness::new();
        h.put(STONE_AT, state("minecraft:stone", &[]));
        let ceiling = STONE_AT + IVec3::new(0, 2, 0);
        h.put(ceiling, state("minecraft:stone", &[]));
        let button = STONE_AT + IVec3::X;
        h.set(
            button,
            state("minecraft:stone_button", &[(FACING, StateValue::Int(5))]),
        );
        let lever = ceiling - IVec3::Y;
        h.set(
            lever,
            state(
                "minecraft:lever",
                &[(LEVER_DIRECTION, StateValue::Str("down_east_west"))],
            ),
        );
        let air = state("minecraft:air", &[]);
        h.set(STONE_AT, air);
        h.set(ceiling, air);
        h.tick(1);
        assert_eq!(h.name(button), "minecraft:air");
        assert_eq!(h.name(lever), "minecraft:air");
        assert_eq!(
            h.drops(),
            vec![
                ("minecraft:lever".to_string(), 1),
                ("minecraft:stone_button".to_string(), 1)
            ]
        );
    }

    #[test]
    fn vines_cling_to_the_clicked_block_and_add_sides() {
        let blocks = stone_on_floor();
        // Clicking the south face: the stone is north of the vine.
        assert_eq!(
            placed(&blocks, "minecraft:vine", STONE_AT, 3, 0.0, VINE_BITS),
            Some((STONE_AT + IVec3::Z, StateValue::Int(4)))
        );
        assert_eq!(
            placed(&blocks, "minecraft:vine", STONE_AT, 5, 0.0, VINE_BITS),
            Some((STONE_AT + IVec3::X, StateValue::Int(2)))
        );
        assert_eq!(
            placed(&blocks, "minecraft:vine", STONE_AT, 1, 0.0, VINE_BITS),
            None
        );
        // A second block east of the vine cell: clicking its west face adds
        // the east side to the vine already there.
        let mut corner = stone_on_floor();
        let cell = STONE_AT + IVec3::Z;
        corner.set(cell + IVec3::X, "minecraft:stone");
        corner.set_state(
            cell,
            state("minecraft:vine", &[(VINE_BITS, StateValue::Int(4))]),
        );
        assert_eq!(
            placed(
                &corner,
                "minecraft:vine",
                cell + IVec3::X,
                4,
                0.0,
                VINE_BITS
            ),
            Some((cell, StateValue::Int(12)))
        );
    }

    #[test]
    fn vines_hang_from_a_vine_above_and_lose_sides_that_stop_holding() {
        let mut h = Harness::new();
        let wall = STONE_AT + IVec3::new(0, 2, 0);
        h.put(wall, state("minecraft:stone", &[]));
        h.put(wall + IVec3::new(1, 0, 1), state("minecraft:stone", &[]));
        // Top vine: on the stone to its north and the one to its east.
        let top = wall + IVec3::Z;
        let vine = |bits| state("minecraft:vine", &[(VINE_BITS, StateValue::Int(bits))]);
        h.set(top, vine(12));
        // A vine below clinging north holds by the vine above alone.
        let below = top - IVec3::Y;
        h.set(below, vine(4));
        assert!(vine_can_survive(
            &h.world,
            below,
            vine(4),
            entry("minecraft:vine")
        ));
        assert!(!vine_can_survive(
            &h.world,
            below,
            vine(1),
            entry("minecraft:vine")
        ));

        h.set(wall + IVec3::new(1, 0, 1), state("minecraft:air", &[]));
        let top_state = h.state(top);
        vine_neighbour_changed(&mut h.world, top, top_state);
        assert_eq!(h.value(top, VINE_BITS), Some(StateValue::Int(4)));

        h.set(wall, state("minecraft:air", &[]));
        h.tick(2);
        assert_eq!(h.name(top), "minecraft:air");
        assert_eq!(h.name(below), "minecraft:air");
    }
}
