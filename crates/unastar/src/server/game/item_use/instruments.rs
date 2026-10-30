//! The spyglass and the goat horn.
//!
//! Measured on BDS 1.26.51 (client-diff `itemuses-misc`). Both are used like
//! a bow is drawn: an air use starts using the held item (the `USINGITEM`
//! flag reaches the player and its viewers) and nothing is consumed. The
//! spyglass sends no sound (the client plays its own) and is used until
//! released or switched away. The goat horn starts the 140-tick
//! `minecraft:goat_horn` cooldown shared by every horn and plays
//! `horn_call<n>` at the player, `n` being its aux value (0 Ponder … 7
//! Dream); a use during the cooldown does nothing. After the horn's use
//! duration (20 ticks in the item data) the server sends the cooldown again
//! and `CompletedUsingItem` (method -1) and stops the use. The sound
//! position (feet + 0.9, where BDS plays bow sounds) is unmeasured.

use bevy_ecs::prelude::*;
use glam::DVec3;

use super::{ItemUsing, UseMethod, start_using};
use crate::entity::components::Position;
use crate::item::ItemStack;

pub const SPYGLASS: &str = "minecraft:spyglass";
pub const GOAT_HORN: &str = "minecraft:goat_horn";
/// Cooldown every goat horn shares.
const HORN_COOLDOWN: (&str, u32) = ("minecraft:goat_horn", 140);
/// Highest goat horn aux (Dream).
const MAX_HORN: i16 = 7;

/// An item used by holding it up, with nothing consumed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Instrument {
    Spyglass,
    GoatHorn,
}

pub fn instrument(identifier: &str) -> Option<Instrument> {
    match identifier {
        SPYGLASS => Some(Instrument::Spyglass),
        GOAT_HORN => Some(Instrument::GoatHorn),
        _ => None,
    }
}

/// The level sound a goat horn of `aux` plays.
pub fn horn_sound(aux: i16) -> Option<String> {
    (0..=MAX_HORN)
        .contains(&aux)
        .then(|| format!("horn_call{aux}"))
}

/// An air use of a spyglass or goat horn.
pub fn use_instrument(
    world: &mut World,
    player: Entity,
    slot: u8,
    stack: &ItemStack,
    instrument: Instrument,
    using: Option<ItemUsing>,
) {
    let Some(item) = stack.item().map(|entry| entry.id) else {
        return;
    };
    if using.is_some() {
        return;
    }
    match instrument {
        Instrument::Spyglass => start_using(world, player, slot, item),
        Instrument::GoatHorn => {
            let (category, ticks) = HORN_COOLDOWN;
            if super::cooldown::cooling(world, player, category) {
                return;
            }
            let Some(call) = horn_sound(stack.damage) else {
                return;
            };
            super::cooldown::start_cooldown(world, player, category, ticks);
            start_using(world, player, slot, item);
            if let Some(eye) = world.get::<Position>(player).map(|p| p.0) {
                let at =
                    eye - DVec3::new(0.0, crate::entity::item_drop::PLAYER_EYE_HEIGHT - 0.9, 0.0);
                crate::entity::mob::net::sound::play_level_sound(
                    world,
                    Some(player),
                    at,
                    &call,
                    -1,
                );
            }
        }
    }
}

/// Finish a goat horn use whose use duration (item data) has passed.
pub fn tick_instrument(world: &mut World, player: Entity, stack: &ItemStack, using: &ItemUsing) {
    let Some(entry) = stack.item().filter(|entry| entry.identifier() == GOAT_HORN) else {
        return;
    };
    let Some(duration) = u64::try_from(entry.data().max_use_duration)
        .ok()
        .filter(|ticks| *ticks > 0)
    else {
        return;
    };
    if super::used_ticks(world, using) >= duration {
        let (category, ticks) = HORN_COOLDOWN;
        super::cooldown::start_cooldown(world, player, category, ticks);
        super::completed(world, player, entry.network_id(), UseMethod::None);
        super::stop_using(world, player);
    }
}
