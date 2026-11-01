//! Composters (`composter_fill_level` 0..=8).
//!
//! - **Composting** ([`add_items`]): an item with a composting chance
//!   (its `minecraft:compostable` component when it has one, otherwise
//!   [`chance`]'s table) is used up whenever the composter is below level 7.
//!   An empty composter always rises to 1; otherwise the level rises when
//!   `nextInt(100) <= chance`. A rise plays `block.composter.fill_success`,
//!   a failed roll `block.composter.fill` (both at the centre, data −1).
//!   Reaching level 7 schedules the composter's tick 20 ticks later.
//! - **Use** (always handled): at level 8 the composter pops one bone meal
//!   from its cell and empties (`block.composter.empty`); below level 7 the
//!   held item is composted: when it was taken the viewers get LevelEvent
//!   2005 at `(x, y + 0.75, z)` (data 0) and the stack loses one item
//!   outside creative. At level 7 nothing happens.
//! - **The tick** turns level 7 into 8 with `block.composter.ready`.
//! - **Comparator output** ([`comparator_signal`]) is the fill level.
//!
//! Hoppers and droppers feeding a composter, hoppers taking its bone meal
//! and the comparator reading it need their own lanes to call
//! [`add_items`], [`take_bone_meal`] and [`comparator_signal`].

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use rand::Rng;

use super::super::{Place, centre, int, random, schedule_tick, with};
use crate::entity::components::GameMode;
use crate::entity::systems::BlockSource;
use crate::item::ItemStack;
use crate::registry::block::{BlockEntry, StateValue};
use crate::server::game::blocks::set_block;
use crate::server::game::item_use::{held, set_slot};

/// Fill level property.
pub const LEVEL: &str = "composter_fill_level";
/// The level at which the composter stops taking items and waits for its
/// tick.
pub const READY_SOON: i32 = 7;
/// The level holding bone meal.
pub const FULL: i32 = 8;
/// Ticks from reaching level 7 to level 8.
pub const READY_DELAY: u64 = 20;
/// LevelEvent of the particles shown when a player's item is composted.
pub const LEVEL_EVENT_COMPOSTED: i32 = 2005;

/// Play the LevelSoundEvent `name` at the centre of `pos` (data −1).
fn sound(world: &mut World, pos: IVec3, name: &str) {
    crate::server::game::blocks::queue_level_sound(world, None, centre(pos), name, -1);
}

/// Composting chance (percent) of the items without a
/// `minecraft:compostable` component; 0 for items that cannot be
/// composted.
pub fn chance(item: &str) -> u32 {
    match item.strip_prefix("minecraft:").unwrap_or(item) {
        "oak_leaves" | "spruce_leaves" | "birch_leaves" | "jungle_leaves" | "acacia_leaves"
        | "dark_oak_leaves" | "azalea_leaves" | "mangrove_leaves" | "cherry_leaves"
        | "pale_oak_leaves" | "oak_sapling" | "spruce_sapling" | "birch_sapling"
        | "jungle_sapling" | "acacia_sapling" | "dark_oak_sapling" | "cherry_sapling"
        | "pale_oak_sapling" | "mangrove_propagule" | "beetroot_seeds" | "melon_seeds"
        | "pumpkin_seeds" | "wheat_seeds" | "torchflower_seeds" | "pitcher_pod"
        | "sweet_berries" | "glow_berries" | "seagrass" | "grass_block" | "kelp" | "dried_kelp"
        | "moss_carpet" | "pale_moss_carpet" | "hanging_roots" | "mangrove_roots"
        | "wildflowers" | "firefly_bush" | "bush" | "leaf_litter" | "cactus_flower"
        | "short_dry_grass" | "tall_dry_grass" => 30,
        "dried_kelp_block"
        | "short_grass"
        | "fern"
        | "cactus"
        | "vine"
        | "weeping_vines"
        | "twisting_vines"
        | "melon_slice"
        | "sugar_cane"
        | "nether_sprouts"
        | "azalea_leaves_flowered"
        | "glow_lichen"
        | "pale_hanging_moss" => 50,
        "carrot"
        | "potato"
        | "wheat"
        | "beetroot"
        | "apple"
        | "nether_wart"
        | "cocoa_beans"
        | "pumpkin"
        | "carved_pumpkin"
        | "melon_block"
        | "red_mushroom"
        | "brown_mushroom"
        | "crimson_fungus"
        | "warped_fungus"
        | "mushroom_stem"
        | "poppy"
        | "blue_orchid"
        | "allium"
        | "azure_bluet"
        | "red_tulip"
        | "orange_tulip"
        | "white_tulip"
        | "pink_tulip"
        | "oxeye_daisy"
        | "cornflower"
        | "lily_of_the_valley"
        | "dandelion"
        | "wither_rose"
        | "open_eyeblossom"
        | "closed_eyeblossom"
        | "pink_petals"
        | "sunflower"
        | "lilac"
        | "tall_grass"
        | "large_fern"
        | "rose_bush"
        | "peony"
        | "crimson_roots"
        | "warped_roots"
        | "sea_pickle"
        | "waterlily"
        | "shroomlight"
        | "azalea"
        | "moss_block"
        | "pale_moss_block"
        | "spore_blossom"
        | "small_dripleaf_block"
        | "big_dripleaf" => 65,
        "hay_block"
        | "brown_mushroom_block"
        | "red_mushroom_block"
        | "nether_wart_block"
        | "warped_wart_block"
        | "bread"
        | "baked_potato"
        | "cookie"
        | "flowering_azalea"
        | "pitcher_plant"
        | "torchflower" => 85,
        "cake" | "pumpkin_pie" => 100,
        _ => 0,
    }
}

/// The composting chance of `stack`: its `minecraft:compostable` component,
/// else [`chance`].
pub fn fill_chance(stack: &ItemStack) -> u32 {
    let Some(entry) = stack.item() else {
        return 0;
    };
    entry
        .components()
        .compostable
        .as_ref()
        .map_or_else(|| chance(entry.identifier()), |c| c.composting_chance)
}

/// Whether a player's `use` on `state` is a block interaction: always.
pub fn is_usable(_entry: &BlockEntry, _state: u32) -> bool {
    true
}

/// A player's `use`: take bone meal at level 8, else compost the held item.
pub fn use_block(world: &mut World, player: Entity, pos: IVec3, state: u32) -> bool {
    let Some(entry) = world.registry().get_by_runtime_id(state).cloned() else {
        return false;
    };
    let level = int(&entry, state, LEVEL).unwrap_or(0);
    if level == FULL {
        take_bone_meal(world, pos);
        return true;
    }
    if level >= READY_SOON {
        return true;
    }
    let Some((slot, stack)) = held(world, player) else {
        return true;
    };
    if add_items(world, pos, &stack, 1) == 1 {
        let at = pos.as_dvec3() + DVec3::new(0.0, 0.75, 0.0);
        crate::entity::mob::net::sound::level_event(world, at, LEVEL_EVENT_COMPOSTED, 0);
        if world.get::<GameMode>(player) != Some(&GameMode::Creative) {
            set_slot(world, player, slot, stack.grow(-1));
        }
    }
    true
}

/// Compost up to `count` items of `stack` into the composter at `pos`;
/// returns how many were used up. Items go in one at a time until the
/// level reaches 7; every item is used up whether or not its roll raised
/// the level.
pub fn add_items(world: &mut World, pos: IVec3, stack: &ItemStack, count: u32) -> u32 {
    let Some(state) = world.block_at(pos) else {
        return 0;
    };
    let Some(entry) = world.registry().get_by_runtime_id(state).cloned() else {
        return 0;
    };
    let chance = fill_chance(stack);
    let old = int(&entry, state, LEVEL).unwrap_or(0);
    if chance == 0 || old >= READY_SOON {
        return 0;
    }
    let mut level = old;
    let mut used = 0;
    while used < count {
        used += 1;
        let rises = level == 0 || random(world).gen_range(0..100) <= chance;
        if rises {
            level += 1;
            if level == READY_SOON {
                break;
            }
        }
    }
    if level == old {
        sound(world, pos, "block.composter.fill");
        return used;
    }
    let new = with(&entry, state, LEVEL, StateValue::Int(level));
    set_block(world, pos, new, false);
    sound(world, pos, "block.composter.fill_success");
    if level == READY_SOON {
        schedule_tick(world, pos, READY_DELAY);
    }
    used
}

/// Take the bone meal of a full composter at `pos`: one bone meal pops out
/// of its cell and the composter empties. Returns whether there was bone
/// meal.
pub fn take_bone_meal(world: &mut World, pos: IVec3) -> bool {
    let Some(state) = world.block_at(pos) else {
        return false;
    };
    let Some(entry) = world.registry().get_by_runtime_id(state).cloned() else {
        return false;
    };
    if int(&entry, state, LEVEL) != Some(FULL) {
        return false;
    }
    if let Some(bone_meal) = crate::server::game::world_stack(world, "minecraft:bone_meal", 1) {
        crate::server::game::block_drops::pop_resource(world, pos, bone_meal);
    }
    let empty = with(&entry, state, LEVEL, StateValue::Int(0));
    set_block(world, pos, empty, false);
    sound(world, pos, "block.composter.empty");
    true
}

/// Comparator output: the fill level.
pub fn comparator_signal(entry: &BlockEntry, state: u32) -> u8 {
    int(entry, state, LEVEL).map_or(0, |level| level.clamp(0, 15) as u8)
}

/// Placement keeps the generic state (an empty composter).
pub fn placement(place: &Place<'_, impl BlockSource>) -> Option<u32> {
    Some(place.state)
}

/// A composter stands anywhere.
pub fn can_survive(
    _blocks: &impl BlockSource,
    _pos: IVec3,
    _state: u32,
    _entry: &BlockEntry,
) -> bool {
    true
}

/// Nothing happens on placement: only composting schedules the tick.
pub fn placed(_world: &mut World, _pos: IVec3, _state: u32) {}

/// Neighbours do not affect a composter.
pub fn neighbour_changed(_world: &mut World, _pos: IVec3, _state: u32, _from: IVec3) {}

/// Level 7 becomes 8, ready to give bone meal.
pub fn scheduled_tick(world: &mut World, pos: IVec3, state: u32) {
    let Some(entry) = world.registry().get_by_runtime_id(state).cloned() else {
        return;
    };
    if int(&entry, state, LEVEL) != Some(READY_SOON) {
        return;
    }
    let ready = with(&entry, state, LEVEL, StateValue::Int(FULL));
    set_block(world, pos, ready, false);
    sound(world, pos, "block.composter.ready");
}

/// Composters have no random tick.
pub fn random_tick(_world: &mut World, _pos: IVec3, _state: u32) {}

/// Redstone power does not act on composters.
pub fn power_changed(_world: &mut World, _pos: IVec3, _state: u32, _powered: bool) {}

/// The compost is not an item: breaking drops only the composter.
pub fn breaking(_world: &mut World, _pos: IVec3, _state: u32) {}

#[cfg(test)]
mod tests {
    use glam::{DVec3, IVec3};
    use jolyne::valentine::McpePacketData;

    use super::super::super::tests::{FLOOR, Harness, state};
    use super::*;

    const AT: IVec3 = IVec3::new(2, FLOOR + 1, 2);

    fn composter(level: i32) -> u32 {
        state("minecraft:composter", &[(LEVEL, StateValue::Int(level))])
    }

    fn level(h: &Harness) -> Option<StateValue> {
        h.value(AT, LEVEL)
    }

    fn sounds(h: &mut Harness) -> Vec<String> {
        h.packets()
            .into_iter()
            .filter_map(|packet| match packet {
                McpePacketData::LevelSoundEventPacket(sound) => Some(sound.sound_event.to_string()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn table_names_registered_items() {
        let h = Harness::new();
        let items = &h
            .world
            .resource::<crate::server::game::types::ItemRegistryResource>()
            .0;
        let compostable = items
            .iter()
            .filter(|item| chance(item.identifier()) > 0)
            .count();
        // Every table row names a registered item (116 rows).
        assert_eq!(compostable, 116);
        assert_eq!(chance("minecraft:cake"), 100);
        assert_eq!(chance("minecraft:stone"), 0);
    }

    #[test]
    fn first_item_always_fills_and_uses_one() {
        let mut h = Harness::new();
        h.put(AT, composter(0));
        h.hold("minecraft:wheat_seeds", 3);
        h.packets();
        h.click(AT, 1, DVec3::new(0.5, 1.0, 0.5));
        assert_eq!(level(&h), Some(StateValue::Int(1)));
        assert!(sounds(&mut h).contains(&"block.composter.fill_success".to_string()));
        let held = crate::server::game::item_use::held(&h.world, h.player)
            .unwrap()
            .1;
        assert_eq!(held.count, 2);
    }

    #[test]
    fn rolls_follow_the_seeded_random_and_always_use_the_item() {
        let mut h = Harness::new();
        h.put(AT, composter(1));
        h.hold("minecraft:wheat_seeds", 64);
        let mut expected = 1;
        // The same seed as the harness: replay the rolls.
        let mut rng = <rand::rngs::StdRng as rand::SeedableRng>::seed_from_u64(7);
        for _ in 0..10 {
            if rng.gen_range(0..100) <= 30 {
                expected += 1;
            }
            h.click(AT, 1, DVec3::new(0.5, 1.0, 0.5));
            assert_eq!(level(&h), Some(StateValue::Int(expected)));
            if expected == READY_SOON {
                break;
            }
        }
        let held = crate::server::game::item_use::held(&h.world, h.player)
            .unwrap()
            .1;
        assert!(held.count < 64);
    }

    #[test]
    fn level_seven_turns_to_bone_meal_after_the_delay_and_empties_on_use() {
        let mut h = Harness::new();
        h.put(AT, composter(6));
        h.hold("minecraft:cake", 1);
        h.click(AT, 1, DVec3::new(0.5, 1.0, 0.5));
        assert_eq!(level(&h), Some(StateValue::Int(READY_SOON)));
        h.tick(READY_DELAY - 1);
        assert_eq!(level(&h), Some(StateValue::Int(READY_SOON)));
        h.packets();
        h.tick(1);
        assert_eq!(level(&h), Some(StateValue::Int(FULL)));
        assert!(sounds(&mut h).contains(&"block.composter.ready".to_string()));
        h.click(AT, 1, DVec3::new(0.5, 1.0, 0.5));
        assert_eq!(level(&h), Some(StateValue::Int(0)));
        assert_eq!(h.drops(), vec![("minecraft:bone_meal".to_string(), 1)]);
    }

    #[test]
    fn uncompostable_items_and_level_seven_take_nothing() {
        let mut h = Harness::new();
        h.put(AT, composter(3));
        h.hold("minecraft:stone", 5);
        h.click(AT, 1, DVec3::new(0.5, 1.0, 0.5));
        assert_eq!(level(&h), Some(StateValue::Int(3)));
        h.put(AT, composter(READY_SOON));
        h.hold("minecraft:cake", 1);
        h.click(AT, 1, DVec3::new(0.5, 1.0, 0.5));
        assert_eq!(level(&h), Some(StateValue::Int(READY_SOON)));
        let held = crate::server::game::item_use::held(&h.world, h.player)
            .unwrap()
            .1;
        assert_eq!(held.count, 1);
    }
}
