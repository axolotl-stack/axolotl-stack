//! The totem of undying.
//!
//! A hit that would kill a player holding a totem in the selected hotbar
//! slot or the offhand (the hotbar first) is survived instead, unless the
//! damage comes from the void: the totem is used up, the player is left
//! with 1 health and no effects, then gets Regeneration II for 900 ticks,
//! Absorption II for 100 ticks and Fire Resistance for 800 ticks, and the
//! player and its viewers get `ActorEvent` `TalismanActivate` (data 0) and
//! `LevelEvent` 1052 at the player. Measured on BDS 1.26.51 (client-diff
//! `itemuses-misc`): the effects, durations, events, health 1 and the
//! offhand / hotbar consumption. The void exception and the hotbar-first
//! order are public references; BDS removes a hotbar totem with an s2c
//! `InventoryTransaction` where this server sends `InventorySlot`.

use bevy_ecs::prelude::*;
use jolyne::valentine::types::{ActorRuntimeId, EnumsActorEvent};
use unastar_data::actor_components::body::ActorDamageCause;

use crate::entity::components::{
    EffectType, Health, HeldSlot, MainInventory, OffhandSlot, Player, RuntimeEntityId,
};
use crate::item::ItemStack;
use crate::server::game::effects::{add_effect, clear_effects};

/// The level event BDS 1.26.51 sends at a player saved by a totem.
const TOTEM_LEVEL_EVENT: i32 = 1052;

pub const TOTEM: &str = "minecraft:totem_of_undying";

/// Effects a used totem gives: effect, amplifier (0-based), ticks.
const TOTEM_EFFECTS: [(EffectType, u8, u32); 3] = [
    (EffectType::REGENERATION, 1, 900),
    (EffectType::ABSORPTION, 1, 100),
    (EffectType::FIRE_RESISTANCE, 0, 800),
];

fn is_totem(stack: &ItemStack) -> bool {
    !stack.is_empty() && stack.item_id() == TOTEM
}

/// The death hook: `player`'s health just reached zero from a hit of
/// `cause`. Returns whether a totem saved it.
pub fn save_from_death(world: &mut World, player: Entity, cause: ActorDamageCause) -> bool {
    if cause == ActorDamageCause::Void || world.get::<Player>(player).is_none() {
        return false;
    }
    let slot = world.get::<HeldSlot>(player).map(|s| usize::from(s.0));
    let in_hotbar = slot.filter(|slot| {
        world
            .get::<MainInventory>(player)
            .and_then(|inv| inv.0.item(*slot))
            .is_some_and(is_totem)
    });
    if let Some(slot) = in_hotbar {
        super::set_slot(world, player, slot as u8, ItemStack::empty());
    } else if world
        .get::<OffhandSlot>(player)
        .is_some_and(|offhand| is_totem(&offhand.0))
    {
        if let Some(mut offhand) = world.get_mut::<OffhandSlot>(player) {
            offhand.0 = ItemStack::empty();
        }
        crate::server::game::inventory::send_offhand(world, player);
    } else {
        return false;
    }
    if let Some(mut health) = world.get_mut::<Health>(player) {
        health.current = 1.0;
    }
    clear_effects(world, player);
    for (effect, amplifier, ticks) in TOTEM_EFFECTS {
        add_effect(world, player, effect, amplifier, ticks);
    }
    if let Some(runtime_id) = world.get::<RuntimeEntityId>(player).map(|r| r.0) {
        let packet = jolyne::valentine::McpePacket::from(jolyne::valentine::ActorEventPacket {
            target_runtime_id: ActorRuntimeId {
                actor_runtime_id: runtime_id as u64,
            },
            event_id: EnumsActorEvent::TalismanActivate,
            data: 0,
            fire_at_position: None,
        });
        super::send_to(world, &super::nearby_players(world, player), &packet);
    }
    if let Some(eye) = world
        .get::<crate::entity::components::Position>(player)
        .map(|p| p.0)
    {
        crate::entity::mob::net::sound::level_event(world, eye, TOTEM_LEVEL_EVENT, 0);
    }
    true
}
