//! Beehives and bee nests (`honey_level` 0..=5, `direction` 0..=3
//! south/west/north/east).
//!
//! - **Placement** turns the entrance towards the placing player
//!   (provisional: Minecraft Wiki).
//! - **Use** acts only on a full hive (level 5), holding shears or a glass
//!   bottle; any other use is not handled:
//!   - shears pop three single honeycombs from the hive's cell and wear by
//!     one, with `block.beehive.shear`;
//!   - a glass bottle becomes a honey bottle (outside creative one bottle
//!     is used up; the honey bottle replaces it or joins the inventory)
//!     with `bucket.fill.water`;
//!   - either way the level drops to 0. Both sounds play at
//!     `(x + 1, y + 1, z + 1)` with data −1.
//! - **Smoke** ([`smoked`]): a lit campfire or soul campfire at most five
//!   blocks below, with only air between, calms the hive. Harvesting an
//!   unsmoked hive angers the bees inside; bees are entities and their
//!   eviction belongs to the entity lane, which can ask [`smoked`].
//! - **Comparator output** ([`comparator_signal`]) is the honey level.

use bevy_ecs::prelude::*;
use glam::IVec3;

use super::super::{Place, bit, int, name_at, with};
use crate::entity::components::GameMode;
use crate::entity::systems::BlockSource;
use crate::registry::block::{BlockEntry, StateValue};
use crate::server::game::blocks::set_block;
use crate::server::game::durability::wear_held;
use crate::server::game::item_use::{give_or_drop, held, set_slot};
use crate::server::game::placement::Horizontal;

/// State properties.
pub const HONEY: &str = "honey_level";
pub const DIRECTION: &str = "direction";
/// A full hive.
pub const FULL: i32 = 5;
/// Honeycombs shears take from a full hive (each popped on its own).
pub const HONEYCOMBS: u32 = 3;
/// How far below a campfire may smoke the hive.
pub const SMOKE_REACH: i32 = 5;

/// Play the harvest sound `name` at `(x + 1, y + 1, z + 1)` (data −1).
fn sound(world: &mut World, pos: IVec3, name: &str) {
    crate::server::game::blocks::queue_level_sound(
        world,
        None,
        (pos + IVec3::ONE).as_dvec3(),
        name,
        -1,
    );
}

/// Whether a lit campfire at most five blocks below the hive at `pos`, with
/// only air between, smokes it.
pub fn smoked(blocks: &impl BlockSource, pos: IVec3) -> bool {
    for depth in 1..=SMOKE_REACH {
        let at = pos - IVec3::new(0, depth, 0);
        let Some(state) = blocks.block_at(at) else {
            return false;
        };
        let Some(entry) = blocks.registry().get_by_runtime_id(state) else {
            return false;
        };
        match entry.string_id.as_str() {
            "minecraft:air" => continue,
            "minecraft:campfire" | "minecraft:soul_campfire" => {
                return !bit(entry, state, "extinguished");
            }
            _ => return false,
        }
    }
    false
}

/// Whether a player's `use` on `state` is a block interaction: a full
/// hive.
pub fn is_usable(entry: &BlockEntry, state: u32) -> bool {
    int(entry, state, HONEY) == Some(FULL)
}

/// A player's `use`: harvest a full hive with shears or a glass bottle.
pub fn use_block(world: &mut World, player: Entity, pos: IVec3, state: u32) -> bool {
    let Some(entry) = world.registry().get_by_runtime_id(state).cloned() else {
        return false;
    };
    if int(&entry, state, HONEY) != Some(FULL) {
        return false;
    }
    let Some((slot, stack)) = held(world, player) else {
        return false;
    };
    match stack.item_id() {
        "minecraft:shears" => {
            for _ in 0..HONEYCOMBS {
                if let Some(comb) =
                    crate::server::game::world_stack(world, "minecraft:honeycomb", 1)
                {
                    crate::server::game::block_drops::pop_resource(world, pos, comb);
                }
            }
            wear_held(world, player, 1);
            sound(world, pos, "block.beehive.shear");
        }
        "minecraft:glass_bottle" => {
            let Some(honey) = crate::server::game::world_stack(world, "minecraft:honey_bottle", 1)
            else {
                return false;
            };
            if world.get::<GameMode>(player) == Some(&GameMode::Creative) {
                give_or_drop(world, player, honey);
            } else {
                let rest = stack.grow(-1);
                if rest.is_empty() {
                    set_slot(world, player, slot, honey);
                } else {
                    set_slot(world, player, slot, rest);
                    give_or_drop(world, player, honey);
                }
            }
            sound(world, pos, "bucket.fill.water");
        }
        _ => return false,
    }
    let empty = with(&entry, state, HONEY, StateValue::Int(0));
    set_block(world, pos, empty, false);
    true
}

/// Comparator output: the honey level.
pub fn comparator_signal(entry: &BlockEntry, state: u32) -> u8 {
    int(entry, state, HONEY).map_or(0, |level| level.clamp(0, FULL) as u8)
}

/// Placement: the entrance faces the placing player.
pub fn placement(place: &Place<'_, impl BlockSource>) -> Option<u32> {
    let facing = Horizontal::from_yaw(place.input.yaw).opposite();
    Some(with(
        place.block,
        place.state,
        DIRECTION,
        StateValue::Int(facing.legacy()),
    ))
}

/// A hive stands anywhere.
pub fn can_survive(
    _blocks: &impl BlockSource,
    _pos: IVec3,
    _state: u32,
    _entry: &BlockEntry,
) -> bool {
    true
}

/// Nothing happens on placement.
pub fn placed(_world: &mut World, _pos: IVec3, _state: u32) {}

/// Neighbours do not affect a hive.
pub fn neighbour_changed(_world: &mut World, _pos: IVec3, _state: u32, _from: IVec3) {}

/// Hives schedule no ticks.
pub fn scheduled_tick(_world: &mut World, _pos: IVec3, _state: u32) {}

/// Hives have no random tick (honey comes from bees, an entity lane).
pub fn random_tick(_world: &mut World, _pos: IVec3, _state: u32) {}

/// Redstone power does not act on hives.
pub fn power_changed(_world: &mut World, _pos: IVec3, _state: u32, _powered: bool) {}

/// A hive holds no items (its bees are entities).
pub fn breaking(_world: &mut World, _pos: IVec3, _state: u32) {}

/// Whether the block at `pos` is a beehive or bee nest (for the entity
/// lane).
pub fn is_hive(blocks: &impl BlockSource, pos: IVec3) -> bool {
    matches!(
        name_at(blocks, pos),
        Some("minecraft:beehive" | "minecraft:bee_nest")
    )
}

#[cfg(test)]
mod tests {
    use glam::{DVec3, IVec3};

    use super::super::super::tests::{FLOOR, Harness, state};
    use super::*;

    const AT: IVec3 = IVec3::new(2, FLOOR + 3, 2);
    const SIDE: DVec3 = DVec3::new(0.5, 0.5, 0.0);

    fn hive(level: i32) -> u32 {
        state("minecraft:beehive", &[(HONEY, StateValue::Int(level))])
    }

    #[test]
    fn shears_take_three_honeycombs_from_a_full_hive() {
        let mut h = Harness::new();
        h.put(AT, hive(FULL));
        h.hold("minecraft:shears", 1);
        h.click(AT, 2, SIDE);
        assert_eq!(h.value(AT, HONEY), Some(StateValue::Int(0)));
        assert_eq!(h.drops(), vec![("minecraft:honeycomb".to_string(), 1); 3]);
        let shears = crate::server::game::item_use::held(&h.world, h.player)
            .unwrap()
            .1;
        assert_eq!(crate::item::enchant::durability_damage(&shears), 1);
    }

    #[test]
    fn a_glass_bottle_takes_a_honey_bottle() {
        let mut h = Harness::new();
        h.put(AT, hive(FULL));
        h.hold("minecraft:glass_bottle", 1);
        h.click(AT, 2, SIDE);
        assert_eq!(h.value(AT, HONEY), Some(StateValue::Int(0)));
        let held = crate::server::game::item_use::held(&h.world, h.player)
            .unwrap()
            .1;
        assert_eq!(held.item_id(), "minecraft:honey_bottle");
    }

    #[test]
    fn a_hive_that_is_not_full_gives_nothing() {
        let mut h = Harness::new();
        h.put(AT, hive(4));
        h.hold("minecraft:shears", 1);
        h.click(AT, 2, SIDE);
        assert_eq!(h.value(AT, HONEY), Some(StateValue::Int(4)));
        assert!(h.drops().is_empty());
    }

    #[test]
    fn a_lit_campfire_below_smokes_the_hive() {
        let mut h = Harness::new();
        h.put(AT, hive(FULL));
        assert!(!smoked(&h.world, AT));
        h.put(AT - IVec3::new(0, 2, 0), state("minecraft:campfire", &[]));
        assert!(smoked(&h.world, AT));
        h.put(
            AT - IVec3::new(0, 2, 0),
            state(
                "minecraft:campfire",
                &[("extinguished", StateValue::Byte(1))],
            ),
        );
        assert!(!smoked(&h.world, AT));
    }

    #[test]
    fn placed_facing_the_player() {
        let mut h = Harness::new();
        h.hold("minecraft:beehive", 1);
        let floor = IVec3::new(2, FLOOR, 2);
        h.click(floor, 1, DVec3::new(0.5, 1.0, 0.5));
        // The player looks south: the entrance faces north.
        assert_eq!(
            h.value(floor + IVec3::Y, DIRECTION),
            Some(StateValue::Int(2))
        );
    }
}
