//! Cakes (`bite_counter` 0..=6) and candle cakes (`<colour>_candle_cake`,
//! `lit`).
//!
//! - **Eating** ([`bite`]): a player may eat when in creative, on peaceful
//!   difficulty or not at full food. A bite feeds 2 food with saturation
//!   modifier 0.1 (+0.4 saturation) and plays `burp` at the player's mouth
//!   (0.1 below the eyes, data −1). A cake below 6 bites gains one bite; the
//!   seventh bite removes it (no drops, no particles). A candle cake drops
//!   its candle and becomes a cake with one bite. A player who may not eat
//!   still has the use handled.
//! - **Candles**: a candle used on an uneaten cake turns it into that
//!   colour's unlit candle cake with `cake.add_candle` at the block's
//!   corner (data −1); the candle is used up outside creative.
//! - **Candle cakes**: flint and steel or a fire charge lights an unlit one
//!   (the item's own lighting rule: flint and steel wears, a fire charge
//!   is used up); an empty hand puts out a lit one with
//!   `extinguish.candle` at the centre (provisional: sound name inferred);
//!   any other use eats.
//! - **Survival**: the block below must block motion (a solid material,
//!   provisional: the material flag is read as "blocks motion"). The core
//!   breaks an unsupported cake one tick after the neighbour change.
//! - **Comparator output** ([`comparator_signal`]): `14 − 2 × bite_counter`;
//!   a candle cake reads as an uneaten cake (provisional).

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};

use super::super::{Place, air, bit, centre, int, with};
use crate::entity::components::{GameMode, Hunger, Position};
use crate::entity::systems::BlockSource;
use crate::registry::block::{BlockEntry, StateValue};
use crate::server::game::blocks::set_block;
use crate::server::game::hunger::{Difficulty, FoodRules};
use crate::server::game::item_use::{held, set_slot};

/// Bites taken.
pub const BITES: &str = "bite_counter";
/// Whether a candle cake's candle burns.
pub const LIT: &str = "lit";
/// The last bite count before the cake is gone.
pub const MAX_BITES: i32 = 6;
/// Food one bite gives.
pub const NUTRITION: i32 = 2;
/// Saturation modifier of one bite.
pub const SATURATION_MODIFIER: f32 = 0.1;

const CAKE: &str = "minecraft:cake";

/// The candle a candle cake carries (`minecraft:red_candle_cake` →
/// `minecraft:red_candle`), `None` for anything else.
pub fn candle_of(cake: &str) -> Option<&str> {
    cake.strip_suffix("_cake")
        .filter(|candle| candle.ends_with("candle"))
}

/// Whether `item` is a candle (`minecraft:candle`, `minecraft:<colour>_candle`).
fn is_candle(item: &str) -> bool {
    item.ends_with(":candle") || item.ends_with("_candle")
}

/// Whether a player's `use` on `state` is a block interaction: always.
pub fn is_usable(_entry: &BlockEntry, _state: u32) -> bool {
    true
}

/// A player's `use`: add a candle, light or put out a candle cake, or eat.
pub fn use_block(world: &mut World, player: Entity, pos: IVec3, state: u32) -> bool {
    let Some(entry) = world.registry().get_by_runtime_id(state).cloned() else {
        return false;
    };
    let held = held(world, player);
    let item = held.as_ref().map(|(_, stack)| stack.item_id().to_owned());
    if entry.string_id == CAKE {
        if let Some((slot, stack)) = &held
            && is_candle(stack.item_id())
            && int(&entry, state, BITES) == Some(0)
        {
            return add_candle(world, player, pos, *slot, stack);
        }
    } else {
        let lit = bit(&entry, state, LIT);
        match item.as_deref() {
            Some("minecraft:flint_and_steel" | "minecraft:fire_charge") => {
                if !lit {
                    crate::server::game::item_use::on_block::use_on_block(world, player, pos, 1);
                }
                return true;
            }
            None if lit => {
                let out = with(&entry, state, LIT, StateValue::Byte(0));
                set_block(world, pos, out, false);
                crate::entity::mob::net::sound::play_level_sound(
                    world,
                    None,
                    centre(pos),
                    "extinguish.candle",
                    -1,
                );
                return true;
            }
            _ => {}
        }
    }
    if may_eat(world, player) {
        bite(world, player, pos);
    }
    true
}

/// Turn the uneaten cake at `pos` into the candle cake of the held candle.
fn add_candle(
    world: &mut World,
    player: Entity,
    pos: IVec3,
    slot: u8,
    stack: &crate::item::ItemStack,
) -> bool {
    let Some(candle_cake) = world
        .registry()
        .default_state_id_by_name(&format!("{}_cake", stack.item_id()))
    else {
        return true;
    };
    crate::entity::mob::net::sound::play_level_sound(
        world,
        None,
        pos.as_dvec3(),
        "cake.add_candle",
        -1,
    );
    set_block(world, pos, candle_cake, false);
    if world.get::<GameMode>(player) != Some(&GameMode::Creative) {
        set_slot(world, player, slot, stack.grow(-1));
    }
    true
}

/// Whether `player` may eat cake: creative, peaceful, or not at full food.
pub fn may_eat(world: &World, player: Entity) -> bool {
    world.get::<GameMode>(player) == Some(&GameMode::Creative)
        || FoodRules::of(world).difficulty == Difficulty::Peaceful
        || world
            .get::<Hunger>(player)
            .is_some_and(|hunger| hunger.food_level < Hunger::MAX_FOOD)
}

/// `player` eats a slice of the cake or candle cake at `pos` (the caller
/// checked [`may_eat`]): feed them, take the bite and burp. Returns whether
/// there was a cake to bite.
pub fn bite(world: &mut World, player: Entity, pos: IVec3) -> bool {
    let Some(state) = world.block_at(pos) else {
        return false;
    };
    let registry = world
        .resource::<crate::server::game::types::BlockRegistryResource>()
        .0
        .clone();
    let Some(entry) = registry.get_by_runtime_id(state) else {
        return false;
    };
    let Some(cake) = registry.get_by_name(CAKE) else {
        return false;
    };
    if entry.string_id == CAKE {
        let bites = int(entry, state, BITES).unwrap_or(0);
        if bites < MAX_BITES {
            let next = with(entry, state, BITES, StateValue::Int(bites + 1));
            set_block(world, pos, next, false);
        } else {
            set_block(world, pos, air(&registry), false);
        }
    } else if let Some(candle) = candle_of(&entry.string_id) {
        if let Some(stack) = crate::server::game::world_stack(world, candle, 1) {
            crate::server::game::block_drops::pop_resource(world, pos, stack);
        }
        let next = with(cake, cake.default_state_id, BITES, StateValue::Int(1));
        set_block(world, pos, next, false);
    } else {
        return false;
    }
    if let Some(mut hunger) = world.get_mut::<Hunger>(player) {
        hunger.eat(NUTRITION, SATURATION_MODIFIER);
    }
    if let Some(eye) = world.get::<Position>(player).map(|p| p.0) {
        crate::entity::mob::net::sound::play_level_sound(
            world,
            Some(player),
            eye - DVec3::new(0.0, 0.1, 0.0),
            "burp",
            -1,
        );
    }
    true
}

/// Comparator output: `14 − 2 × bite_counter` (14 for a candle cake).
pub fn comparator_signal(entry: &BlockEntry, state: u32) -> u8 {
    let bites = int(entry, state, BITES).unwrap_or(0).clamp(0, 7);
    (14 - 2 * bites) as u8
}

/// Placement keeps the generic state (an uneaten cake).
pub fn placement(place: &Place<'_, impl BlockSource>) -> Option<u32> {
    Some(place.state)
}

/// A cake needs a block below that blocks motion.
pub fn can_survive(
    blocks: &impl BlockSource,
    pos: IVec3,
    _state: u32,
    _entry: &BlockEntry,
) -> bool {
    blocks
        .block_at(pos - IVec3::Y)
        .and_then(|below| blocks.registry().get_by_runtime_id(below))
        .is_some_and(|below| below.data.blocks_motion)
}

/// Nothing happens on placement.
pub fn placed(_world: &mut World, _pos: IVec3, _state: u32) {}

/// Survival is checked by the core; nothing else reacts to neighbours.
pub fn neighbour_changed(_world: &mut World, _pos: IVec3, _state: u32, _from: IVec3) {}

/// Cakes schedule no ticks of their own (the core's survival tick breaks
/// an unsupported one).
pub fn scheduled_tick(_world: &mut World, _pos: IVec3, _state: u32) {}

/// Cakes have no random tick.
pub fn random_tick(_world: &mut World, _pos: IVec3, _state: u32) {}

/// Redstone power does not act on cakes.
pub fn power_changed(_world: &mut World, _pos: IVec3, _state: u32, _powered: bool) {}

/// A cake holds no items.
pub fn breaking(_world: &mut World, _pos: IVec3, _state: u32) {}

#[cfg(test)]
mod tests {
    use glam::{DVec3, IVec3};
    use jolyne::valentine::McpePacketData;

    use super::super::super::tests::{FLOOR, Harness, state};
    use super::*;

    const AT: IVec3 = IVec3::new(2, FLOOR + 1, 2);
    const TOP: DVec3 = DVec3::new(0.5, 0.5, 0.5);

    fn hungry(h: &mut Harness, food: i32) {
        h.world.entity_mut(h.player).insert(Hunger {
            food_level: food,
            saturation: 0.0,
            exhaustion: 0.0,
        });
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
    fn seven_bites_eat_the_cake_and_feed_the_player() {
        let mut h = Harness::new();
        h.put(AT, state(CAKE, &[]));
        hungry(&mut h, 2);
        h.packets();
        for bites in 1..=6 {
            h.click(AT, 1, TOP);
            assert_eq!(h.value(AT, BITES), Some(StateValue::Int(bites)));
        }
        assert!(sounds(&mut h).contains(&"burp".to_string()));
        h.click(AT, 1, TOP);
        assert_eq!(h.name(AT), "minecraft:air");
        let hunger = h.world.get::<Hunger>(h.player).unwrap();
        assert_eq!(hunger.food_level, 16);
        assert!((hunger.saturation - 2.8).abs() < 1e-4);
        assert!(h.drops().is_empty());
    }

    #[test]
    fn a_full_player_cannot_eat_but_creative_can() {
        let mut h = Harness::new();
        h.put(AT, state(CAKE, &[]));
        hungry(&mut h, Hunger::MAX_FOOD);
        h.click(AT, 1, TOP);
        assert_eq!(h.value(AT, BITES), Some(StateValue::Int(0)));
        *h.world.get_mut::<GameMode>(h.player).unwrap() = GameMode::Creative;
        h.click(AT, 1, TOP);
        assert_eq!(h.value(AT, BITES), Some(StateValue::Int(1)));
    }

    #[test]
    fn a_candle_makes_a_candle_cake_that_drops_it_when_eaten() {
        let mut h = Harness::new();
        h.put(AT, state(CAKE, &[]));
        hungry(&mut h, 10);
        h.hold("minecraft:red_candle", 2);
        h.packets();
        h.click(AT, 1, TOP);
        assert_eq!(h.name(AT), "minecraft:red_candle_cake");
        assert!(sounds(&mut h).contains(&"cake.add_candle".to_string()));
        let held = crate::server::game::item_use::held(&h.world, h.player)
            .unwrap()
            .1;
        assert_eq!(held.count, 1);
        h.hold("minecraft:flint_and_steel", 1);
        h.click(AT, 1, TOP);
        assert_eq!(h.value(AT, LIT), Some(StateValue::Byte(1)));
        h.hold("minecraft:stick", 1);
        h.click(AT, 1, TOP);
        assert_eq!(h.name(AT), CAKE);
        assert_eq!(h.value(AT, BITES), Some(StateValue::Int(1)));
        assert_eq!(h.drops(), vec![("minecraft:red_candle".to_string(), 1)]);
    }

    #[test]
    fn a_candle_needs_an_uneaten_cake() {
        let mut h = Harness::new();
        h.put(AT, state(CAKE, &[(BITES, StateValue::Int(2))]));
        hungry(&mut h, 10);
        h.hold("minecraft:candle", 1);
        h.click(AT, 1, TOP);
        assert_eq!(h.name(AT), CAKE);
        assert_eq!(h.value(AT, BITES), Some(StateValue::Int(3)));
    }

    #[test]
    fn a_cake_breaks_without_the_block_below() {
        let mut h = Harness::new();
        let cake = AT + IVec3::Y;
        h.put(AT, state("minecraft:stone", &[]));
        h.put(cake, state(CAKE, &[]));
        h.set(AT, state("minecraft:air", &[]));
        assert_eq!(h.name(cake), CAKE);
        h.tick(1);
        assert_eq!(h.name(cake), "minecraft:air");
        assert!(h.drops().is_empty());
    }
}
