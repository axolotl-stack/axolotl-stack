//! Beds: two blocks, the foot and the head (`head_piece_bit`), lying along
//! `direction` (0 south, 1 west, 2 north, 3 east), as BDS 1.26.51 does.
//!
//! - **Placement**: the foot goes in the target cell and the head one block
//!   further in the direction the player looks, which becomes the bed's
//!   `direction`. Both cells must be free and not lava, and the blocks below
//!   both must hold the bed on their top face (a full top, or the top of a
//!   fence or wall).
//! - **Survival**: each half needs its partner (the same bed block, the
//!   other `head_piece_bit`, the same `direction`). Support below is only
//!   checked when the bed is placed. A half left alone is removed by the
//!   neighbour update at once, without drops (quiet destroy particles).
//! - **Breaking** either half by a player removes the partner without drops
//!   and yields the broken half's own drop, so a bed drops one bed item
//!   whichever half breaks.
//!
//! The bed's colour lives in its block entity, which the server does not
//! keep yet: placed beds show the default colour and drop a plain bed.

use glam::IVec3;

use super::{Place, SupportKind, bit, int, name_at, provides_support, with};
use crate::entity::systems::BlockSource;
use crate::registry::block::{BlockEntry, StateValue};
use crate::server::game::placement::Horizontal;

/// Bed state properties.
pub const DIRECTION: &str = "direction";
pub const HEAD: &str = "head_piece_bit";
pub const OCCUPIED: &str = "occupied_bit";

/// The direction a bed state lies along (foot to head).
pub fn direction(entry: &BlockEntry, state: u32) -> Option<Horizontal> {
    int(entry, state, DIRECTION).and_then(Horizontal::from_legacy)
}

/// The partner cell of the bed half `state` at `pos`.
fn partner(entry: &BlockEntry, pos: IVec3, state: u32) -> Option<IVec3> {
    let step = direction(entry, state)?.offset();
    Some(if bit(entry, state, HEAD) {
        pos - step
    } else {
        pos + step
    })
}

fn is_lava(blocks: &impl BlockSource, pos: IVec3) -> bool {
    matches!(
        name_at(blocks, pos),
        Some("minecraft:lava" | "minecraft:flowing_lava")
    )
}

pub fn can_survive(blocks: &impl BlockSource, pos: IVec3, state: u32, entry: &BlockEntry) -> bool {
    let Some(other_pos) = partner(entry, pos, state) else {
        return false;
    };
    let head = bit(entry, state, HEAD);
    let facing = direction(entry, state);
    blocks.block_at(other_pos).is_some_and(|other| {
        (entry.min_state_id..=entry.max_state_id).contains(&other)
            && bit(entry, other, HEAD) != head
            && direction(entry, other) == facing
    })
}

pub fn placement(place: &Place<'_, impl BlockSource>) -> Option<u32> {
    let blocks = place.blocks;
    let facing = Horizontal::from_yaw(place.input.yaw);
    let foot = place.position;
    let head = foot + facing.offset();
    let supported = [foot, head].into_iter().all(|cell| {
        !is_lava(blocks, cell)
            && provides_support(blocks, cell - IVec3::Y, IVec3::Y, SupportKind::Centre)
    });
    if !supported {
        return None;
    }
    let block = place.block;
    let state = with(
        block,
        place.state,
        DIRECTION,
        StateValue::Int(facing.legacy()),
    );
    let state = with(block, state, HEAD, StateValue::Byte(0));
    Some(with(block, state, OCCUPIED, StateValue::Byte(0)))
}

/// The head placed with the foot `state` at `position`.
pub fn parts(entry: &BlockEntry, position: IVec3, state: u32) -> Vec<(IVec3, u32)> {
    if bit(entry, state, HEAD) {
        return Vec::new();
    }
    partner(entry, position, state)
        .map(|head| (head, with(entry, state, HEAD, StateValue::Byte(1))))
        .into_iter()
        .collect()
}

/// The other half of the bed half `state` at `pos`, and the state whose
/// drops a player's break yields (the broken half itself).
pub fn structure(entry: &BlockEntry, pos: IVec3, state: u32) -> (Vec<IVec3>, u32) {
    (partner(entry, pos, state).into_iter().collect(), state)
}

#[cfg(test)]
mod tests {
    use glam::DVec3;

    use super::super::tests::{FLOOR, Harness, state};
    use super::*;
    use crate::entity::systems::physics::tests::TestBlocks;
    use crate::registry::BlockRegistry;
    use crate::server::game::placement::{Placement, PlacementInput, resolve_placement};

    const FOOT: IVec3 = IVec3::new(0, FLOOR + 1, 0);

    fn bed() -> &'static BlockEntry {
        BlockRegistry::vanilla()
            .get_by_name("minecraft:bed")
            .expect("bed")
    }

    /// Place a bed on the top face of the floor under [`FOOT`], looking
    /// along `yaw`.
    fn place(blocks: &TestBlocks, yaw: f32) -> Option<Placement> {
        let input = PlacementInput {
            clicked: FOOT - IVec3::Y,
            face: 1,
            click: DVec3::new(0.5, 1.0, 0.5),
            yaw,
            pitch: 30.0,
        };
        resolve_placement(blocks, bed(), &input)
    }

    fn floor() -> TestBlocks {
        TestBlocks::floor("minecraft:stone", FLOOR)
    }

    fn bed_state(facing: Horizontal, head: bool) -> u32 {
        state(
            "minecraft:bed",
            &[
                (DIRECTION, StateValue::Int(facing.legacy())),
                (HEAD, StateValue::Byte(u8::from(head))),
            ],
        )
    }

    #[test]
    fn the_head_goes_one_block_further_in_the_look_direction() {
        for (yaw, facing) in [
            (0.0, Horizontal::South),
            (90.0, Horizontal::West),
            (180.0, Horizontal::North),
            (270.0, Horizontal::East),
        ] {
            let placement = place(&floor(), yaw).unwrap();
            assert_eq!(placement.position, FOOT);
            assert_eq!(placement.state, bed_state(facing, false), "yaw {yaw}");
            assert_eq!(
                placement.parts,
                vec![(FOOT + facing.offset(), bed_state(facing, true))],
                "yaw {yaw}"
            );
        }
    }

    #[test]
    fn beds_are_refused_without_room_or_support_for_the_head() {
        let mut blocked = floor();
        blocked.set(FOOT + IVec3::Z, "minecraft:stone");
        assert_eq!(place(&blocked, 0.0), None);
        let mut hole = floor();
        hole.set(FOOT + IVec3::new(0, -1, 1), "minecraft:air");
        assert_eq!(place(&hole, 0.0), None);
        let mut slab = floor();
        slab.set(FOOT + IVec3::new(0, -1, 1), "minecraft:smooth_stone_slab");
        assert_eq!(place(&slab, 0.0), None);
        let mut lava = floor();
        lava.set(FOOT + IVec3::Z, "minecraft:lava");
        assert_eq!(place(&lava, 0.0), None);
    }

    #[test]
    fn a_half_without_its_partner_goes_without_drops() {
        let mut h = Harness::new();
        let head = FOOT + IVec3::NEG_X;
        h.put(FOOT, bed_state(Horizontal::West, false));
        h.put(head, bed_state(Horizontal::West, true));
        h.set(head, state("minecraft:air", &[]));
        assert_eq!(h.name(FOOT), "minecraft:air");
        assert!(h.drops().is_empty());
    }

    #[test]
    fn a_bed_needs_its_partner_in_the_same_direction() {
        let mut blocks = floor();
        let foot = bed_state(Horizontal::North, false);
        blocks.set_state(FOOT, foot);
        blocks.set_state(FOOT + IVec3::NEG_Z, bed_state(Horizontal::North, true));
        assert!(can_survive(&blocks, FOOT, foot, bed()));
        blocks.set_state(FOOT + IVec3::NEG_Z, bed_state(Horizontal::South, true));
        assert!(!can_survive(&blocks, FOOT, foot, bed()));
        blocks.set_state(FOOT + IVec3::NEG_Z, bed_state(Horizontal::North, false));
        assert!(!can_survive(&blocks, FOOT, foot, bed()));
    }

    #[test]
    fn breaking_either_half_yields_one_bed() {
        for broken_head in [false, true] {
            let mut h = Harness::new();
            let head = FOOT + IVec3::Z;
            h.put(FOOT, bed_state(Horizontal::South, false));
            h.put(head, bed_state(Horizontal::South, true));
            let (broken, other) = if broken_head {
                (head, FOOT)
            } else {
                (FOOT, head)
            };
            let broken_state = h.state(broken);
            let drop_state = super::super::break_structure(&mut h.world, broken, broken_state);
            assert_eq!(drop_state, broken_state);
            assert_eq!(h.name(other), "minecraft:air");
            let drops: Vec<_> =
                crate::server::game::block_drops::block_drops(&mut h.world, drop_state, None)
                    .into_iter()
                    .map(|stack| (stack.item_id().to_string(), stack.count))
                    .collect();
            assert_eq!(drops, vec![("minecraft:bed".to_string(), 1)]);
        }
    }
}
