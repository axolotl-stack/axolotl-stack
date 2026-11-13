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
/// buttons popping back up, placed coral blocks checking for water.
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

/// A live coral block placed by a player dies at a random tick within 60
/// unless water touches it (BDS 1.26.51: 12 placements out of water were
/// dead by tick 60, two of them by tick 10; a coral block set without a
/// player stays alive).
pub(crate) fn placed(world: &mut World, pos: IVec3) {
    let Some((entry, _)) = entry_at(world, pos) else {
        return;
    };
    let name = entry.string_id.as_str();
    if name.ends_with("_coral_block") && !name.starts_with("minecraft:dead_") {
        use rand::Rng;
        let ticks = world
            .get_resource_mut::<crate::entity::EntityRandom>()
            .map_or(30, |mut random| random.0.gen_range(1..=60));
        schedule(world, pos, ticks);
    }
}

fn touches_water(world: &World, pos: IVec3) -> bool {
    super::block_update::NEIGHBOURS.iter().any(|offset| {
        entry_at(world, pos + *offset).is_some_and(|(entry, _)| {
            matches!(
                entry.string_id.as_str(),
                "minecraft:water" | "minecraft:flowing_water"
            )
        })
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

/// Pops pressed buttons back up when their time is up (exclusive: edits
/// chunks). Runs once per server tick.
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
