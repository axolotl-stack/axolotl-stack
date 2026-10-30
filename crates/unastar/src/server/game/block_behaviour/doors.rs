//! Doors: two blocks (`upper_block_bit`) with a direction
//! (`minecraft:cardinal_direction`), a hinge (`door_hinge_bit`) and an open
//! state (`open_bit`), as BDS 1.26.51 stores them.
//!
//! - **Direction**: a door's direction runs along its closed panel (a closed
//!   door facing south has its panel against the west side of the cell, see
//!   the collision table). A placed door turns its panel across the
//!   player's way, on the player's side of the cell: the direction is the
//!   player's look turned 90° clockwise (looking north places an east
//!   door), as the BDS 1.26.51 capture shows.
//! - **Placement** needs the cell and the one above it free and, below,
//!   a block whose top gives edge support (a full top face, farmland or a
//!   dirt path). The lower half carries the direction and the open state;
//!   the upper half carries the direction and the hinge.
//! - **Hinge**: with `d` the door's direction, the cells at `-d` and `+d`
//!   (both levels) decide. A door on the `-d` side and none on the `+d`
//!   side sets the hinge bit (two doors placed side by side form a double
//!   door); otherwise the bit is set when fewer full blocks stand on the
//!   `-d` side than on the `+d` side. The click position plays no part.
//! - **Use** flips `open_bit` on the lower half (whichever half was used),
//!   resends the upper half and plays `door.open` / `door.close` at the used
//!   half for the viewers, carrying the lower half's state from before the
//!   toggle; iron doors ignore hands (redstone only), so a block in hand is
//!   placed against them instead. BDS sends both halves in one
//!   UpdateSubChunkBlocks packet; unastar sends two UpdateBlock packets.
//! - **Power** (wooden and iron doors) opens the door when either half
//!   becomes powered and closes it when the door loses power, as a use
//!   would, with the same sounds ([`set_open`], driven by
//!   [`super::redstone`]).
//! - **Survival**: the lower half needs its support and its upper half;
//!   the upper half needs its lower half. A half left alone breaks with its
//!   drops one tick after the neighbour update (only the lower half
//!   carries the door item).
//! - **Breaking** either half by a player removes both at once; the break
//!   yields the lower half's drops.

use bevy_ecs::prelude::*;
use glam::IVec3;

use super::{Place, SupportKind, bit, provides_support, with};
use crate::entity::systems::BlockSource;
use crate::registry::block::{BlockEntry, StateValue};
use crate::server::game::placement::Horizontal;

/// Door state properties.
pub const HINGE: &str = "door_hinge_bit";
pub const OPEN: &str = "open_bit";
pub const UPPER_BIT: &str = "upper_block_bit";
pub const DIRECTION: &str = "minecraft:cardinal_direction";

/// `LevelSoundEvent` names a door plays when it opens or closes.
pub const SOUND_OPEN: &str = "door.open";
pub const SOUND_CLOSE: &str = "door.close";

/// The direction a door state has.
pub fn direction(entry: &BlockEntry, state: u32) -> Option<Horizontal> {
    match entry.state_value(state, DIRECTION)? {
        StateValue::Str(value) => Horizontal::from_cardinal(value),
        _ => None,
    }
}

fn is_door(blocks: &impl BlockSource, pos: IVec3) -> bool {
    blocks.block_at(pos).is_some_and(|state| {
        blocks
            .registry()
            .get_by_runtime_id(state)
            .is_some_and(|entry| entry.state_value(state, HINGE).is_some())
    })
}

fn full_block(blocks: &impl BlockSource, pos: IVec3) -> bool {
    super::NEIGHBOURS
        .iter()
        .all(|face| provides_support(blocks, pos, *face, SupportKind::Full))
}

/// The hinge bit of a door with direction `direction` placed at `pos` (see
/// the module docs).
fn hinge(blocks: &impl BlockSource, pos: IVec3, direction: Horizontal) -> bool {
    let step = direction.offset();
    let side = |offset: IVec3| [pos + offset, pos + offset + IVec3::Y];
    let doors = |offset: IVec3| side(offset).iter().any(|p| is_door(blocks, *p));
    let solid = |offset: IVec3| {
        side(offset)
            .iter()
            .filter(|p| full_block(blocks, **p))
            .count()
    };
    if doors(-step) && !doors(step) {
        return true;
    }
    solid(-step) < solid(step)
}

/// Placement state of a door (lower half).
pub fn placement(place: &Place<'_, impl BlockSource>) -> Option<u32> {
    let direction = Horizontal::from_yaw(place.input.yaw).clockwise();
    let block = place.block;
    let mut state = with(
        block,
        place.state,
        DIRECTION,
        StateValue::Str(direction.cardinal()),
    );
    state = with(block, state, UPPER_BIT, StateValue::Byte(0));
    state = with(block, state, OPEN, StateValue::Byte(0));
    // The hinge is stored on the upper half only (see `parts`).
    state = with(block, state, HINGE, StateValue::Byte(0));
    Some(state)
}

/// The upper half placed with the lower half `state` at `position`: same
/// direction, closed, with the hinge decided by the neighbours.
pub fn parts(
    blocks: &impl BlockSource,
    entry: &BlockEntry,
    position: IVec3,
    state: u32,
) -> Vec<(IVec3, u32)> {
    if bit(entry, state, UPPER_BIT) {
        return Vec::new();
    }
    let Some(direction) = direction(entry, state) else {
        return Vec::new();
    };
    let mut upper = with(entry, state, UPPER_BIT, StateValue::Byte(1));
    upper = with(entry, upper, OPEN, StateValue::Byte(0));
    upper = with(
        entry,
        upper,
        HINGE,
        StateValue::Byte(u8::from(hinge(blocks, position, direction))),
    );
    vec![(position + IVec3::Y, upper)]
}

pub fn can_survive(blocks: &impl BlockSource, pos: IVec3, state: u32, entry: &BlockEntry) -> bool {
    let same = |p: IVec3, upper: bool| {
        blocks.block_at(p).is_some_and(|other| {
            (entry.min_state_id..=entry.max_state_id).contains(&other)
                && bit(entry, other, UPPER_BIT) == upper
        })
    };
    if bit(entry, state, UPPER_BIT) {
        same(pos - IVec3::Y, false)
    } else {
        same(pos + IVec3::Y, true)
            && provides_support(blocks, pos - IVec3::Y, IVec3::Y, SupportKind::Edge)
    }
}

/// The other half of the door half `state` at `pos`, and the state whose
/// drops a player's break yields (the lower half).
pub fn structure(entry: &BlockEntry, pos: IVec3, state: u32) -> (Vec<IVec3>, u32) {
    let upper = bit(entry, state, UPPER_BIT);
    let partner = if upper {
        pos - IVec3::Y
    } else {
        pos + IVec3::Y
    };
    (
        vec![partner],
        with(entry, state, UPPER_BIT, StateValue::Byte(0)),
    )
}

/// Open or close the door with a half at `pos` and play the door sound.
/// Returns whether the door toggled.
pub fn toggle(world: &mut World, pos: IVec3, state: u32, _player: Option<Entity>) -> bool {
    let registry = world
        .resource::<crate::server::game::types::BlockRegistryResource>()
        .0
        .clone();
    let Some(entry) = registry.get_by_runtime_id(state) else {
        return false;
    };
    let lower = if bit(entry, state, UPPER_BIT) {
        pos - IVec3::Y
    } else {
        pos
    };
    let Some(lower_state) = world
        .block_at(lower)
        .filter(|s| (entry.min_state_id..=entry.max_state_id).contains(s))
    else {
        return false;
    };
    let open = !bit(entry, lower_state, OPEN);
    let next = with(entry, lower_state, OPEN, StateValue::Byte(u8::from(open)));
    crate::server::game::blocks::set_block(world, lower, next, false);
    // The upper half keeps its state; viewers get it again with the change.
    let upper = lower + IVec3::Y;
    if let Some(upper_state) = world.block_at(upper) {
        crate::server::game::blocks::set_block(world, upper, upper_state, false);
    }
    // The sound carries the lower half as it was before the toggle.
    super::sound(
        world,
        pos,
        if open { SOUND_OPEN } else { SOUND_CLOSE },
        lower_state,
    );
    true
}

/// Open (`open`) or close the door with its lower half `state` at `pos`,
/// as a use would; nothing when it already is. Returns whether it moved.
pub fn set_open(world: &mut World, pos: IVec3, state: u32, open: bool) -> bool {
    let is_open = world
        .registry()
        .get_by_runtime_id(state)
        .is_some_and(|entry| bit(entry, state, OPEN));
    is_open != open && toggle(world, pos, state, None)
}

/// A mob broke the door half at `cell` (see `annotation.break_door`): the
/// half goes without drops, with quiet destroy particles of the door's
/// default state; the other half, left alone, breaks a tick later with its
/// drops and destroy particles. Returns whether a door half was there.
pub fn break_by_mob(world: &mut World, cell: IVec3) -> bool {
    let Some(state) = world.block_at(cell) else {
        return false;
    };
    let registry = world
        .resource::<crate::server::game::types::BlockRegistryResource>()
        .0
        .clone();
    let Some(entry) = registry.get_by_runtime_id(state) else {
        return false;
    };
    if entry.state_value(state, HINGE).is_none() {
        return false;
    }
    super::particles(
        world,
        cell,
        entry.default_state_id,
        super::LEVEL_EVENT_PARTICLES_DESTROY_BLOCK_NO_SOUND,
    );
    crate::server::game::blocks::set_block(world, cell, super::air(&registry), false);
    true
}

#[cfg(test)]
mod tests {
    use glam::{DVec3, IVec3};
    use jolyne::valentine::McpePacketData;

    use super::super::tests::{FLOOR, Harness, state};
    use super::*;
    use crate::entity::components::GameMode;

    const LOWER: IVec3 = IVec3::new(0, FLOOR + 1, 0);
    const UPPER: IVec3 = IVec3::new(0, FLOOR + 2, 0);

    /// A player south of the door cell looking north places `item` on the
    /// floor below `cell`.
    fn place_door(h: &mut Harness, item: &str, cell: IVec3) {
        h.stand(DVec3::new(0.5, 65.62, 3.5), 180.0, 30.0);
        h.hold(item, 4);
        h.click(cell - IVec3::Y, 1, DVec3::new(0.5, 1.0, 0.5));
    }

    /// Door sounds sent: `true` for `door.open`, `false` for `door.close`.
    fn door_sounds(packets: &[McpePacketData]) -> Vec<bool> {
        packets
            .iter()
            .filter_map(|p| match p {
                McpePacketData::LevelSoundEventPacket(e) if e.sound_event == SOUND_OPEN => {
                    Some(true)
                }
                McpePacketData::LevelSoundEventPacket(e) if e.sound_event == SOUND_CLOSE => {
                    Some(false)
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn placed_door_takes_both_halves_across_the_players_way() {
        let mut h = Harness::new();
        place_door(&mut h, "minecraft:wooden_door", LOWER);
        assert_eq!(h.name(LOWER), "minecraft:wooden_door");
        assert_eq!(h.name(UPPER), "minecraft:wooden_door");
        assert_eq!(h.value(LOWER, UPPER_BIT), Some(StateValue::Byte(0)));
        assert_eq!(h.value(UPPER, UPPER_BIT), Some(StateValue::Byte(1)));
        for half in [LOWER, UPPER] {
            // Looking north: the panel spans x on the player's (south) side.
            assert_eq!(h.value(half, DIRECTION), Some(StateValue::Str("east")));
            assert_eq!(h.value(half, OPEN), Some(StateValue::Byte(0)));
            assert_eq!(h.value(half, HINGE), Some(StateValue::Byte(0)));
            let boxes: Vec<_> =
                crate::registry::collision::block_collision_aabbs(h.state(half)).collect();
            assert_eq!(boxes.len(), 1);
            assert!(boxes[0].min.z > 0.8 && boxes[0].max.x == 1.0, "{boxes:?}");
        }
    }

    #[test]
    fn doors_beside_a_door_or_more_walls_set_the_hinge_on_the_upper_half() {
        // An east door's sides are west (-x) and east (+x). A second door
        // east of the first has a door on its west side.
        let mut h = Harness::new();
        place_door(&mut h, "minecraft:wooden_door", LOWER);
        place_door(&mut h, "minecraft:wooden_door", LOWER + IVec3::X);
        assert_eq!(h.value(UPPER + IVec3::X, HINGE), Some(StateValue::Byte(1)));
        assert_eq!(h.value(LOWER + IVec3::X, HINGE), Some(StateValue::Byte(0)));
        // The first door has a door on its east side only: no hinge bit.
        assert_eq!(h.value(UPPER, HINGE), Some(StateValue::Byte(0)));

        let stone = state("minecraft:stone", &[]);
        for (side, hinge) in [(IVec3::NEG_X, 0), (IVec3::X, 1)] {
            let mut walls = Harness::new();
            walls.put(LOWER + side, stone);
            walls.put(UPPER + side, stone);
            place_door(&mut walls, "minecraft:wooden_door", LOWER);
            assert_eq!(
                walls.value(UPPER, HINGE),
                Some(StateValue::Byte(hinge)),
                "{side}"
            );
        }
    }

    #[test]
    fn door_needs_room_and_a_supporting_floor() {
        let mut h = Harness::new();
        h.put(UPPER, state("minecraft:stone", &[]));
        place_door(&mut h, "minecraft:wooden_door", LOWER);
        assert_eq!(h.name(LOWER), "minecraft:air");

        let mut slab = Harness::new();
        slab.put(IVec3::new(0, FLOOR, 0), state("minecraft:oak_slab", &[]));
        place_door(&mut slab, "minecraft:wooden_door", LOWER);
        assert_eq!(
            slab.name(LOWER),
            "minecraft:air",
            "a bottom slab has no full top"
        );

        let mut farmland = Harness::new();
        farmland.put(IVec3::new(0, FLOOR, 0), state("minecraft:farmland", &[]));
        place_door(&mut farmland, "minecraft:wooden_door", LOWER);
        assert_eq!(farmland.name(LOWER), "minecraft:wooden_door");
    }

    #[test]
    fn door_toggles_on_every_click_from_either_half() {
        let mut h = Harness::new();
        place_door(&mut h, "minecraft:wooden_door", LOWER);
        let upper_state = h.state(UPPER);
        h.hold("minecraft:dirt", 1);
        h.packets();
        for (click, open) in [(LOWER, 1), (UPPER, 0), (UPPER, 1), (LOWER, 0)] {
            h.click(click, 3, DVec3::new(0.5, 0.5, 1.0));
            assert_eq!(
                h.value(LOWER, OPEN),
                Some(StateValue::Byte(open)),
                "{click}"
            );
            assert_eq!(
                h.state(UPPER),
                upper_state,
                "the upper half keeps its state"
            );
            let packets = h.packets();
            assert_eq!(door_sounds(&packets), vec![open == 1], "{click}");
            let updated: Vec<IVec3> = packets
                .iter()
                .filter_map(|p| match p {
                    McpePacketData::UpdateBlockPacket(u) => Some(IVec3::new(
                        u.block_position.x,
                        u.block_position.y,
                        u.block_position.z,
                    )),
                    _ => None,
                })
                .collect();
            assert!(
                updated.contains(&LOWER) && updated.contains(&UPPER),
                "{updated:?}"
            );
        }
        assert_eq!(
            h.name(LOWER + IVec3::Z),
            "minecraft:air",
            "the dirt was not placed"
        );
    }

    #[test]
    fn iron_doors_ignore_hands() {
        let mut h = Harness::new();
        place_door(&mut h, "minecraft:iron_door", LOWER);
        assert_eq!(h.name(UPPER), "minecraft:iron_door");
        h.hold("minecraft:stick", 1);
        h.packets();
        h.click(LOWER, 3, DVec3::new(0.5, 0.5, 1.0));
        assert_eq!(h.value(LOWER, OPEN), Some(StateValue::Byte(0)));
        assert!(door_sounds(&h.packets()).is_empty());
    }

    #[test]
    fn breaking_either_half_removes_the_whole_door_and_drops_one() {
        for (hit, mode, drops) in [
            (UPPER, GameMode::Survival, 1),
            (LOWER, GameMode::Survival, 1),
            (UPPER, GameMode::Creative, 0),
            (LOWER, GameMode::Creative, 0),
        ] {
            let mut h = Harness::new();
            place_door(&mut h, "minecraft:wooden_door", LOWER);
            *h.world.get_mut::<GameMode>(h.player).unwrap() = mode;
            h.updates();
            h.break_at(hit);
            assert_eq!(h.name(LOWER), "minecraft:air", "{hit} {mode:?}");
            assert_eq!(h.name(UPPER), "minecraft:air", "{hit} {mode:?}");
            let updates = h.updates();
            let air = state("minecraft:air", &[]);
            for half in [LOWER, UPPER] {
                assert_eq!(
                    updates
                        .iter()
                        .rev()
                        .find(|(pos, _)| *pos == half)
                        .map(|u| u.1),
                    Some(air),
                    "the client is told both halves are gone ({hit} {mode:?})"
                );
            }
            h.tick(2);
            let expected: Vec<(String, u8)> = (0..drops)
                .map(|_| ("minecraft:wooden_door".to_string(), 1))
                .collect();
            assert_eq!(h.drops(), expected, "{hit} {mode:?}");
        }
    }

    #[test]
    fn door_breaks_with_its_drop_a_tick_after_the_floor_goes() {
        let mut h = Harness::new();
        place_door(&mut h, "minecraft:wooden_door", LOWER);
        h.set(LOWER - IVec3::Y, state("minecraft:air", &[]));
        assert_eq!(
            h.name(LOWER),
            "minecraft:wooden_door",
            "not before the tick"
        );
        h.tick(1);
        assert_eq!(h.name(LOWER), "minecraft:air");
        h.tick(1);
        assert_eq!(h.name(UPPER), "minecraft:air");
        assert_eq!(h.drops(), vec![("minecraft:wooden_door".to_string(), 1)]);
    }

    #[test]
    fn a_mob_removes_its_half_quietly_and_the_other_half_breaks_next_tick() {
        let mut h = Harness::new();
        place_door(&mut h, "minecraft:wooden_door", LOWER);
        h.packets();
        assert!(break_by_mob(&mut h.world, LOWER));
        assert_eq!(h.name(LOWER), "minecraft:air");
        h.tick(1);
        assert_eq!(h.name(UPPER), "minecraft:air");
        assert!(h.drops().is_empty(), "the upper half carries no item");
        let ids = crate::server::game::types::block_network_ids(&h.world);
        let door = crate::registry::BlockRegistry::vanilla()
            .get_by_name("minecraft:wooden_door")
            .unwrap();
        let events: Vec<(i32, i32)> = h
            .packets()
            .into_iter()
            .filter_map(|p| match p {
                McpePacketData::LevelEventPacket(e) => Some((e.event_id, e.data)),
                _ => None,
            })
            .collect();
        assert_eq!(
            events.first(),
            Some(&(
                super::super::LEVEL_EVENT_PARTICLES_DESTROY_BLOCK_NO_SOUND,
                ids.to_wire(door.default_state_id) as i32
            ))
        );
        assert!(
            events
                .iter()
                .any(|(id, _)| *id == super::super::LEVEL_EVENT_PARTICLES_DESTROY_BLOCK)
        );
        assert!(!break_by_mob(&mut h.world, LOWER), "nothing left to break");
    }
}
