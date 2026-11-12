//! Blocks a click opens, closes or presses, as BDS 1.26.51 does them
//! (`testdata/bds-placements-uses.json`, the `uses` group of
//! `tools/bds-sweep/placements.py`; every door, trapdoor, fence gate and
//! button measured once, plus variations).
//!
//! - A door (not iron) opens and closes whichever half is clicked and
//!   whatever the player holds; only the lower half's `open_bit` changes (the
//!   upper half's is not used). Sound `door.open` / `door.close` at the
//!   clicked half's centre with the lower half's state before the click.
//! - A trapdoor (not iron) toggles `open_bit`: `trapdoor.open` / `.close`.
//! - A fence gate opens and closes; opened from the side it faces away from
//!   it turns to face the way the player looks: `fence_gate.open` / `.close`.
//! - A button is pressed (`button.click_on`) and released 20 ticks later
//!   (stone and polished blackstone) or 30 ticks later (wooden ones), with
//!   `button.click_off`; a pressed button ignores the click.
//! - A lever toggles `open_bit`: `button.click_on` / `button.click_off`.
//!
//! BDS answers these clicks with the player's inventory and no block
//! corrections; the clicking player may not be sent the door it opened (its
//! client predicted it), the others are.

use std::collections::BTreeMap;

use bevy_ecs::prelude::*;
use glam::IVec3;
use jolyne::valentine::{LevelSoundEventPacket, McpePacket, Vec3};

use super::block_actors;
use super::placement::Horizontal;
use super::types::BlockRegistryResource;
use crate::entity::components::Rotation;
use crate::registry::block::{BlockEntry, StateValue};

/// Block changes waiting for their tick (of [`ScheduledBlocks::now`]):
/// buttons popping back up, coral blocks checking for water.
#[derive(Resource, Debug, Default)]
pub(crate) struct ScheduledBlocks {
    now: u64,
    due: BTreeMap<u64, Vec<IVec3>>,
}

fn schedule(world: &mut World, pos: IVec3, ticks: u64) {
    let mut scheduled = world.get_resource_or_insert_with(ScheduledBlocks::default);
    let due = scheduled.now + ticks;
    scheduled.due.entry(due).or_default().push(pos);
}

/// Ticks after a live coral block is placed out of water (by a player, set
/// with a block update or by `/setblock`) until BDS 1.26.51 checks it: 45 in
/// all 23 tick-by-tick cells (`block_updates/coral_timing.json`).
pub(crate) const CORAL_PLACED_TICKS: u64 = 45;

/// Ticks after a neighbour of a live coral block changes, leaving it with no
/// water, until it is checked: BDS 1.26.51 killed a coral block whose water
/// was removed 54 to 184 ticks later (15 tick-by-tick cells,
/// `block_updates/coral_timing.json`; 10 more read every 10 ticks,
/// `coral_water.json`). The draw is uniform over that range: the cells do
/// not pin down BDS's own distribution.
pub(crate) const CORAL_DRY_TICKS: std::ops::RangeInclusive<u64> = 54..=184;

/// Whether a live coral block is at `pos` (runs on every block change: no
/// entry clones).
fn live_coral_block(world: &World, pos: IVec3) -> bool {
    let Some(state) = super::blocks::block_at(world, (pos.x, pos.y, pos.z)) else {
        return false;
    };
    world
        .get_resource::<BlockRegistryResource>()
        .and_then(|registry| registry.0.get_by_runtime_id(state))
        .is_some_and(|entry| {
            entry.string_id.ends_with("_coral_block")
                && !entry.string_id.starts_with("minecraft:dead_")
        })
}

/// A block at `pos` changed (`placed`: a block was set there). A live coral
/// block placed there with no water touching it is checked
/// [`CORAL_PLACED_TICKS`] later; a live coral block beside the change with no
/// water touching it is checked after a draw from [`CORAL_DRY_TICKS`]. A
/// check finding no water kills it (`tick_scheduled_blocks`). A coral block
/// placed beside water is not checked until a neighbour changes (BDS 1.26.51,
/// `block_updates/coral_water.json`).
pub(crate) fn coral_changed(world: &mut World, pos: IVec3, placed: bool) {
    if placed && live_coral_block(world, pos) && !touches_water(world, pos) {
        schedule(world, pos, CORAL_PLACED_TICKS);
    }
    for offset in super::block_update::NEIGHBOURS {
        let at = pos + offset;
        if live_coral_block(world, at) && !touches_water(world, at) {
            use rand::Rng;
            let ticks = world
                .get_resource_or_init::<crate::entity::EntityRandom>()
                .0
                .gen_range(CORAL_DRY_TICKS);
            schedule(world, at, ticks);
        }
    }
}

/// Water beside `pos`: a water block, or water in a block's liquid layer.
fn touches_water(world: &World, pos: IVec3) -> bool {
    super::block_update::NEIGHBOURS.iter().any(|offset| {
        let at = pos + *offset;
        let water = |state: u32| {
            world
                .get_resource::<BlockRegistryResource>()
                .and_then(|registry| registry.0.liquid_state(state))
                .is_some_and(|liquid| liquid.kind == crate::registry::block::LiquidKind::Water)
        };
        super::blocks::block_at(world, (at.x, at.y, at.z)).is_some_and(water)
            || super::blocks::layer_at(world, at).is_some_and(water)
    })
}

fn entry_at(world: &World, pos: IVec3) -> Option<(BlockEntry, u32)> {
    let state = super::blocks::block_at(world, (pos.x, pos.y, pos.z))?;
    let entry = world
        .get_resource::<BlockRegistryResource>()?
        .0
        .get_by_runtime_id(state)?
        .clone();
    Some((entry, state))
}

fn byte(entry: &BlockEntry, state: u32, property: &str) -> Option<bool> {
    match entry.state_value(state, property)? {
        StateValue::Byte(value) => Some(value != 0),
        _ => None,
    }
}

fn with_byte(entry: &BlockEntry, state: u32, property: &str, value: bool) -> u32 {
    entry
        .with_state_value(state, property, StateValue::Byte(u8::from(value)))
        .unwrap_or(state)
}

fn sound(world: &World, pos: IVec3, name: &str, state: u32) {
    let data = super::types::block_network_ids(world).to_wire(state) as i32;
    block_actors::broadcast(
        world,
        pos,
        &McpePacket::from(LevelSoundEventPacket {
            sound_event: name.into(),
            position: Vec3 {
                x: pos.x as f32 + 0.5,
                y: pos.y as f32 + 0.5,
                z: pos.z as f32 + 0.5,
            },
            data,
            actor_identifier: "".into(),
            is_baby: false,
            is_global: false,
            actor_unique_id: -1,
            fire_at_position: None,
        }),
        None,
    );
}

fn held_empty(world: &World, player: Entity) -> bool {
    use crate::entity::components::{HeldSlot, MainInventory};
    let Some(slot) = world.get::<HeldSlot>(player).map(|slot| slot.0) else {
        return true;
    };
    world
        .get::<MainInventory>(player)
        .and_then(|inventory| inventory.hotbar(slot))
        .is_none_or(|stack| stack.is_empty())
}

/// The identifier of what `player` holds, if anything.
fn held_id(world: &World, player: Entity) -> Option<String> {
    use crate::entity::components::{HeldSlot, MainInventory};
    let slot = world.get::<HeldSlot>(player)?.0;
    let stack = world.get::<MainInventory>(player)?.hotbar(slot)?;
    (!stack.is_empty()).then(|| stack.item_id().to_owned())
}

/// Bites a cake has before it is gone.
const CAKE_SLICES: i32 = 7;

/// `player` eats a slice of the cake (or candle cake) at `pos` (BDS 1.26.51,
/// `testdata/bds-placements-candles.json` and `-leftovers.json`): the cake
/// takes one more bite or, after its last slice, is gone; a candle cake
/// drops its candle and keeps one bite; the player gains the slice's food
/// and burps. A player with a full hunger bar eats too.
fn eat_cake(world: &mut World, player: Entity, pos: IVec3, entry: &BlockEntry, state: u32) {
    use crate::entity::components::{Hunger, Position};
    let bites = match entry.state_value(state, "bite_counter") {
        Some(StateValue::Int(bites)) => bites,
        _ => 0,
    };
    let Some(registry) = world
        .get_resource::<BlockRegistryResource>()
        .map(|registry| registry.0.clone())
    else {
        return;
    };
    let next = if bites + 1 >= CAKE_SLICES {
        registry.default_state_id_by_name("minecraft:air")
    } else {
        registry.get_by_name("minecraft:cake").and_then(|cake| {
            cake.with_state_value(
                cake.default_state_id,
                "bite_counter",
                StateValue::Int(bites + 1),
            )
        })
    };
    let Some(next) = next else {
        return;
    };
    super::blocks::set_block(world, pos, next, false);
    if let Some(candle) = entry.string_id.strip_suffix("_cake")
        && entry.string_id != "minecraft:cake"
        && let Some(stack) = world
            .get_resource::<super::types::ItemRegistryResource>()
            .and_then(|items| items.0.stack(candle, 1))
    {
        super::block_drops::pop_resource(world, pos, stack);
    }
    if let Some(mut hunger) = world.get_mut::<Hunger>(player) {
        hunger.eat(CAKE_SLICE_FOOD.0, CAKE_SLICE_FOOD.1);
    }
    if let Some(eye) = world.get::<Position>(player).map(|position| position.0) {
        crate::entity::mob::net::sound::play_level_sound(
            world,
            Some(player),
            eye - glam::DVec3::new(0.0, 0.1, 0.0),
            "burp",
            -1,
        );
    }
}

/// Food points and saturation modifier of one cake slice (BDS 1.26.51: a
/// hungry player went from 12 food and 0.4 saturation to 14 and 0.8,
/// `cake_hungry_normal` in `testdata/bds-placements-leftovers.json`).
pub(crate) const CAKE_SLICE_FOOD: (i32, f32) = (2, 0.1);

/// Whether `player` may eat cake: below full food, or on peaceful, where
/// BDS 1.26.51 lets a full player eat (`cake_empty_hand` on peaceful and
/// `cake_full_normal` on normal difficulty). Unmeasured in creative.
fn can_eat_cake(world: &World, player: Entity) -> bool {
    use crate::entity::components::Hunger;
    super::hunger::FoodRules::of(world).difficulty == super::hunger::Difficulty::Peaceful
        || world
            .get::<Hunger>(player)
            .is_none_or(|hunger| hunger.food_level < Hunger::MAX_FOOD)
}

fn is_iron(entry: &BlockEntry) -> bool {
    entry.string_id.starts_with("minecraft:iron_")
}

/// Ticks a pressed button stays down: wooden buttons 30, stone ones 20.
fn button_ticks(entry: &BlockEntry) -> u64 {
    match entry.string_id.as_str() {
        "minecraft:stone_button" | "minecraft:polished_blackstone_button" => 20,
        _ => 30,
    }
}

/// A player clicked `pos`. `None` when the block there does not react to a
/// click (the click goes on to place what is held); `Some(false)` when it
/// did (no block corrections follow).
pub(crate) fn used(world: &mut World, player: Entity, pos: IVec3) -> Option<bool> {
    let (entry, state) = entry_at(world, pos)?;
    let id = entry.string_id.as_str();
    if entry.state_value(state, "door_hinge_bit").is_some() {
        if is_iron(&entry) {
            return None;
        }
        let lower = if byte(&entry, state, "upper_block_bit") == Some(true) {
            pos - IVec3::Y
        } else {
            pos
        };
        let (lower_entry, lower_state) = entry_at(world, lower)?;
        if lower_entry.string_id != entry.string_id {
            return None;
        }
        let open = byte(&lower_entry, lower_state, "open_bit") == Some(true);
        let next = with_byte(&lower_entry, lower_state, "open_bit", !open);
        super::blocks::set_block(world, lower, next, false);
        // BDS sends both halves (the upper one unchanged) and plays the sound
        // with the lower half as it was.
        if let Some(upper) = super::blocks::block_at(world, (lower.x, lower.y + 1, lower.z)) {
            super::blocks::write_block(world, lower + IVec3::Y, upper, false);
        }
        let name = if open { "door.close" } else { "door.open" };
        sound(world, pos, name, lower_state);
        return Some(false);
    }
    if entry.has_tag("trapdoors") || id.ends_with("_trapdoor") || id == "minecraft:trapdoor" {
        if is_iron(&entry) {
            return None;
        }
        let open = byte(&entry, state, "open_bit") == Some(true);
        let next = with_byte(&entry, state, "open_bit", !open);
        super::blocks::set_block(world, pos, next, false);
        sound(
            world,
            pos,
            if open {
                "trapdoor.close"
            } else {
                "trapdoor.open"
            },
            next,
        );
        return Some(false);
    }
    if entry.state_value(state, "in_wall_bit").is_some() {
        let open = byte(&entry, state, "open_bit") == Some(true);
        let mut next = with_byte(&entry, state, "open_bit", !open);
        if !open {
            // Opened from the side it faces away from, it turns to face the
            // way the player looks.
            let look = world
                .get::<Rotation>(player)
                .map_or(Horizontal::South, |r| Horizontal::from_yaw(r.yaw));
            let facing = match entry.state_value(state, "minecraft:cardinal_direction") {
                Some(StateValue::Str(facing)) => facing,
                _ => "south",
            };
            if facing == look.opposite().cardinal() {
                next = entry
                    .with_state_value(
                        next,
                        "minecraft:cardinal_direction",
                        StateValue::Str(look.cardinal()),
                    )
                    .unwrap_or(next);
            }
        }
        super::blocks::set_block(world, pos, next, false);
        sound(
            world,
            pos,
            if open {
                "fence_gate.close"
            } else {
                "fence_gate.open"
            },
            next,
        );
        return Some(false);
    }
    if id.ends_with("_button") {
        if byte(&entry, state, "button_pressed_bit") == Some(true) {
            return Some(false);
        }
        let next = with_byte(&entry, state, "button_pressed_bit", true);
        super::blocks::set_block(world, pos, next, false);
        sound(world, pos, "button.click_on", next);
        schedule(world, pos, button_ticks(&entry));
        return Some(false);
    }
    // A lit candle or candle cake goes out under an empty hand (BDS 1.26.51,
    // `testdata/bds-placements-candles.json`).
    if (id.ends_with("candle") || id.ends_with("candle_cake"))
        && byte(&entry, state, "lit") == Some(true)
        && held_empty(world, player)
    {
        let next = with_byte(&entry, state, "lit", false);
        super::blocks::set_block(world, pos, next, false);
        block_actors::broadcast(
            world,
            pos,
            &McpePacket::from(LevelSoundEventPacket {
                sound_event: "extinguish.candle".into(),
                position: Vec3 {
                    x: pos.x as f32,
                    y: pos.y as f32,
                    z: pos.z as f32,
                },
                data: -1,
                actor_identifier: "".into(),
                is_baby: false,
                is_global: false,
                actor_unique_id: -1,
                fire_at_position: None,
            }),
            None,
        );
        return Some(false);
    }
    // A cake loses a slice to any click but a candle onto a whole cake
    // (which the placement turns into a candle cake), if the player may eat.
    if id == "minecraft:cake" || id.ends_with("candle_cake") {
        let whole = entry.state_value(state, "bite_counter") == Some(StateValue::Int(0));
        if whole && held_id(world, player).is_some_and(|held| held.ends_with("candle")) {
            return None;
        }
        if can_eat_cake(world, player) {
            eat_cake(world, player, pos, &entry, state);
        }
        return Some(false);
    }
    if id == "minecraft:lever" {
        let on = byte(&entry, state, "open_bit") == Some(true);
        let next = with_byte(&entry, state, "open_bit", !on);
        super::blocks::set_block(world, pos, next, false);
        sound(
            world,
            pos,
            if on {
                "button.click_off"
            } else {
                "button.click_on"
            },
            next,
        );
        return Some(false);
    }
    None
}

/// Pops pressed buttons back up when their time is up and kills the coral
/// blocks whose check finds no water (exclusive: edits chunks). Runs once
/// per server tick.
pub(crate) fn tick_scheduled_blocks(world: &mut World) {
    let due = {
        let mut scheduled = world.get_resource_or_insert_with(ScheduledBlocks::default);
        scheduled.now += 1;
        let now = scheduled.now;
        let later = scheduled.due.split_off(&(now + 1));
        std::mem::replace(&mut scheduled.due, later)
    };
    for pos in due.into_values().flatten() {
        let Some((entry, state)) = entry_at(world, pos) else {
            continue;
        };
        let name = entry.string_id.clone();
        if name.ends_with("_coral_block") && !name.starts_with("minecraft:dead_") {
            if !touches_water(world, pos) {
                let dead = name.replacen("minecraft:", "minecraft:dead_", 1);
                if let Some(dead) = world
                    .get_resource::<BlockRegistryResource>()
                    .and_then(|registry| registry.0.default_state_id_by_name(&dead))
                {
                    super::blocks::set_block(world, pos, dead, false);
                }
            }
            continue;
        }
        if byte(&entry, state, "button_pressed_bit") != Some(true) {
            continue;
        }
        let next = with_byte(&entry, state, "button_pressed_bit", false);
        super::blocks::set_block(world, pos, next, false);
        sound(world, pos, "button.click_off", next);
    }
}
