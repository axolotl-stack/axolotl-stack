//! Player interactions with mobs: spawn eggs, feeding babies, love mode and
//! authored `minecraft:interact` rows (milking).
//!
//! [`player_interact`] tries, in order: the mob's own spawn egg on a mob with
//! `minecraft:spawn_egg_interaction` (spawns a baby), `minecraft:ageable`
//! items on a baby (feed, pause, resume), `minecraft:breedable` items on an
//! adult (love mode), then the `minecraft:interact` rows. The first that
//! applies uses the held item (one is consumed outside creative) and makes
//! the mob [`Persistent`]. [`use_spawn_egg_on_block`] is the block-face
//! spawn egg path.

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use jolyne::valentine::actor_flags;
use jolyne::valentine::types::{ActorRuntimeId, EnumsActorEvent, Vec3};
use jolyne::valentine::{
    ActorEventPacket, ItemUseOnActorInventoryTransaction, LevelSoundEventPacket, McpePacket,
};
use rand::Rng;
use tracing::warn;

use super::ageing::BabyAge;
use super::config::{
    AgeableConfig, BreedableConfig, InteractConfig, ItemMatcher, SpawnEggInteraction,
};
use super::events::{MobFacts, held_item};
use super::state::{BreedCooldown, Dead, InLove, Persistent};
use super::{MobKind, SpawnCause, constants, spawn_mob};
use crate::ecs::resources::TickCounter;
use crate::entity::EntityRandom;
use crate::entity::components::{
    ActorViewers, GameMode, HeldSlot, MainInventory, PlayerSession, Position, RuntimeId,
};
use crate::entity::flags::ActorFlags;
use crate::entity::metadata::EntityMetadata;
use crate::item::ItemStack;
use crate::server::game::{send_slot, world_stack};

/// A player uses their held item on `target`.
pub fn player_interact(
    world: &mut World,
    player: Entity,
    target: Entity,
    transaction: &ItemUseOnActorInventoryTransaction,
) {
    if world.get::<Dead>(target).is_some() {
        return;
    }
    let (Some(mode), Some(slot)) = (
        world.get::<GameMode>(player).copied(),
        world.get::<HeldSlot>(player).map(|slot| slot.0),
    ) else {
        return;
    };
    if mode == GameMode::Spectator {
        return;
    }
    if transaction.slot != i32::from(slot) {
        resend_held(world, player);
        return;
    }
    let Some(held) = held_item(world, player).cloned() else {
        return;
    };
    let used = spawn_baby_from_egg(world, player, target, &held)
        || ageable_interaction(world, player, target, &held)
        || fall_in_love(world, player, target, &held)
        || authored_interaction(world, player, target);
    if used {
        world.entity_mut(target).insert(Persistent);
    }
}

/// The actor a spawn egg item creates: `minecraft:cow_spawn_egg` →
/// `minecraft:cow`.
pub fn spawn_egg_actor(item_id: &str) -> Option<&str> {
    let actor = item_id.strip_suffix("_spawn_egg")?;
    let (namespace, name) = actor.split_once(':')?;
    (!namespace.is_empty() && !name.is_empty()).then_some(actor)
}

/// A spawn egg used on a block face: the mob spawns standing at the bottom
/// centre of the block adjacent to the clicked face (on top of the block
/// for the top face), with a random facing, and one egg is used outside
/// creative. Returns whether the held item was a spawn egg (the click is
/// then fully handled, even when the actor cannot be spawned).
pub fn use_spawn_egg_on_block(world: &mut World, player: Entity, adjacent: IVec3) -> bool {
    let Some(actor) = held_item(world, player)
        .and_then(|held| spawn_egg_actor(&held.item_id))
        .map(str::to_owned)
    else {
        return false;
    };
    let feet = DVec3::new(
        f64::from(adjacent.x) + 0.5,
        f64::from(adjacent.y),
        f64::from(adjacent.z) + 0.5,
    );
    let yaw = world
        .resource_mut::<EntityRandom>()
        .0
        .gen_range(0.0f32..360.0);
    match spawn_mob(world, &actor, feet, yaw, SpawnCause::Spawned) {
        Ok(_) => use_held_item(world, player, None),
        Err(error) => {
            warn!(%actor, %error, "spawn egg could not spawn its actor");
            resend_held(world, player);
        }
    }
    true
}

/// `minecraft:spawn_egg_interaction`: the mob's own spawn egg spawns a baby
/// of its kind at its position (the `entity_born` path).
fn spawn_baby_from_egg(
    world: &mut World,
    player: Entity,
    target: Entity,
    held: &ItemStack,
) -> bool {
    let Some(actor) = spawn_egg_actor(&held.item_id) else {
        return false;
    };
    if world.get::<SpawnEggInteraction>(target).is_none() {
        return false;
    }
    let Some(kind) = world.get::<MobKind>(target) else {
        return false;
    };
    if kind.0.identifier.as_ref() != actor {
        return false;
    }
    let identifier = std::sync::Arc::clone(&kind.0.identifier);
    let Some(position) = world.get::<Position>(target).map(|p| p.0) else {
        return false;
    };
    match spawn_mob(world, &identifier, position, 0.0, SpawnCause::Born) {
        Ok(_) => {
            use_held_item(world, player, None);
            true
        }
        Err(error) => {
            warn!(actor = %identifier, %error, "spawn egg could not spawn a baby");
            false
        }
    }
}

/// `minecraft:ageable` on a baby: feed items advance growth, pause items
/// stop it and reset items let it grow again.
fn ageable_interaction(
    world: &mut World,
    player: Entity,
    target: Entity,
    held: &ItemStack,
) -> bool {
    let (Some(config), Some(mut age)) = (
        world.get::<AgeableConfig>(target).cloned(),
        world.get::<BabyAge>(target).copied(),
    ) else {
        return false;
    };
    let now = world.resource::<TickCounter>().current;
    let matches = |items: &[ItemMatcher]| {
        items
            .iter()
            .any(|item| item.matches(&held.item_id, held.damage))
    };
    let fed = config
        .feed_items
        .iter()
        .find(|entry| entry.item.matches(&held.item_id, held.damage))
        .is_some_and(|entry| age.feed(&config, entry.growth));
    let used = fed
        || (matches(&config.pause_growth_items) && age.pause(now))
        || (matches(&config.reset_growth_items) && age.resume(now));
    if !used {
        return false;
    }
    world.entity_mut(target).insert(age);
    if fed {
        broadcast_actor_event(world, target, EnumsActorEvent::BabyAge, 0);
    }
    use_held_item(world, player, None);
    true
}

/// `minecraft:breedable`: a breed item puts an adult that is neither in
/// love nor cooling down into love mode for [`constants::LOVE_TICKS`].
fn fall_in_love(world: &mut World, player: Entity, target: Entity, held: &ItemStack) -> bool {
    let Some(config) = world.get::<BreedableConfig>(target) else {
        return false;
    };
    let now = world.resource::<TickCounter>().current;
    // Taming is not implemented, so a mob that must be tamed never breeds.
    if config.require_tame
        || !config
            .breed_items
            .iter()
            .any(|item| item.matches(&held.item_id, held.damage))
        || world
            .get::<InLove>(target)
            .is_some_and(|love| now < love.until_tick)
        || world
            .get::<BreedCooldown>(target)
            .is_some_and(|cooldown| now < cooldown.until_tick)
        || is_baby(world, target)
    {
        return false;
    }
    world.entity_mut(target).insert(InLove {
        until_tick: now + u64::from(constants::LOVE_TICKS),
        player: Some(player),
    });
    set_love_flag(world, target, true);
    broadcast_actor_event(world, target, EnumsActorEvent::InLoveHearts, 0);
    use_held_item(world, player, None);
    true
}

/// The first `minecraft:interact` row whose filter passes (other = the
/// player): uses the held item, hands out `transform_to_item` and plays the
/// row's sounds.
fn authored_interaction(world: &mut World, player: Entity, target: Entity) -> bool {
    let Some(config) = world.get::<InteractConfig>(target) else {
        return false;
    };
    let interactions = std::sync::Arc::clone(&config.interactions);
    let facts = MobFacts {
        world,
        entity: target,
        other: Some(player),
    };
    let Some(row) = interactions.iter().find(|row| {
        row.filter
            .as_ref()
            .is_none_or(|filter| filter.evaluate(&facts))
    }) else {
        return false;
    };
    let result = match &row.transform_to_item {
        Some(matcher) => {
            let Some(stack) = modern_item(matcher).and_then(|id| world_stack(world, id, 1)) else {
                warn!(item = %matcher.identifier, aux = ?matcher.aux,
                    "interaction result item is not in the item registry");
                return false;
            };
            Some(stack)
        }
        None => None,
    };
    if row.use_item || result.is_some() {
        use_held_item_with(world, player, row.use_item, result);
    }
    for sound in row.play_sounds.iter() {
        play_mob_sound(world, target, sound);
    }
    true
}

/// The flattened identifier of an authored item name; legacy `bucket:<aux>`
/// names resolve through [`constants::LEGACY_BUCKET_ITEMS`].
fn modern_item(matcher: &ItemMatcher) -> Option<&str> {
    if matcher.identifier.as_ref() == "minecraft:bucket" {
        let aux = matcher.aux.unwrap_or(0);
        return constants::LEGACY_BUCKET_ITEMS
            .iter()
            .find(|(legacy, _)| *legacy == aux)
            .map(|(_, item)| *item);
    }
    matches!(matcher.aux, None | Some(0)).then_some(&matcher.identifier)
}

fn is_baby(world: &World, entity: Entity) -> bool {
    world
        .get::<ActorFlags>(entity)
        .is_some_and(|flags| flags.get(actor_flags::BABY).unwrap_or(false))
}

/// Use one of the held item (kept in creative).
fn use_held_item(world: &mut World, player: Entity, result: Option<ItemStack>) {
    use_held_item_with(world, player, true, result);
}

/// Optionally use one of the held item (never in creative) and hand the
/// player `result`: it replaces a used-up held stack, otherwise it goes into
/// the inventory (dropped at the player when full). Creative players only
/// receive `result` when they hold none of it yet. Changed slots are resent.
fn use_held_item_with(world: &mut World, player: Entity, consume: bool, result: Option<ItemStack>) {
    let creative = world.get::<GameMode>(player) == Some(&GameMode::Creative);
    let (Some(slot), Some(mut inventory)) = (
        world
            .get::<HeldSlot>(player)
            .map(|slot| usize::from(slot.0)),
        world.get_mut::<MainInventory>(player),
    ) else {
        return;
    };
    let before = inventory.0.slots().to_vec();
    if consume
        && !creative
        && let Some(held) = inventory.0.item_mut(slot)
    {
        *held = held.grow(-1);
    }
    let mut leftover = None;
    if let Some(result) = result {
        let held_empty = inventory.0.item(slot).is_none_or(ItemStack::is_empty);
        if held_empty {
            let _ = inventory.0.set_item(slot, result);
        } else if !creative
            || inventory
                .0
                .first(|stack| stack.item_id == result.item_id)
                .is_none()
        {
            let (_, rest) = inventory.0.add_item(result);
            leftover = Some(rest).filter(|rest| !rest.is_empty());
        }
    }
    let changed: Vec<(usize, ItemStack)> = inventory
        .0
        .slots()
        .iter()
        .zip(&before)
        .enumerate()
        .filter(|(_, (now, was))| now != was)
        .map(|(index, (now, _))| (index, now.clone()))
        .collect();
    for (index, stack) in &changed {
        send_slot(world, player, *index, stack);
    }
    if let Some(rest) = leftover
        && let Some(position) = world.get::<Position>(player).map(|p| p.0)
    {
        crate::entity::spawn_item_drop(world, position, rest, Some(player));
    }
}

/// Correct the client's view of the held slot after a refused interaction.
fn resend_held(world: &World, player: Entity) {
    let Some(slot) = world
        .get::<HeldSlot>(player)
        .map(|slot| usize::from(slot.0))
    else {
        return;
    };
    let Some(stack) = world
        .get::<MainInventory>(player)
        .and_then(|inventory| inventory.0.item(slot))
    else {
        return;
    };
    send_slot(world, player, slot, stack);
}

/// Send an actor event about `mob` to its viewers.
pub(crate) fn broadcast_actor_event(world: &World, mob: Entity, event: EnumsActorEvent, data: i32) {
    let (Some(runtime_id), Some(viewers)) =
        (world.get::<RuntimeId>(mob), world.get::<ActorViewers>(mob))
    else {
        return;
    };
    let packet = ActorEventPacket {
        target_runtime_id: ActorRuntimeId {
            actor_runtime_id: runtime_id.0 as u64,
        },
        event_id: event,
        data,
        fire_at_position: None,
    };
    for viewer in &viewers.viewers {
        if let Some(session) = world.get::<PlayerSession>(*viewer) {
            let _ = session.send(McpePacket::from(packet.clone()));
        }
    }
}

/// Play a named level sound at `mob` for its viewers.
fn play_mob_sound(world: &World, mob: Entity, sound: &str) {
    let (Some(kind), Some(position), Some(viewers)) = (
        world.get::<MobKind>(mob),
        world.get::<Position>(mob),
        world.get::<ActorViewers>(mob),
    ) else {
        return;
    };
    let packet = LevelSoundEventPacket {
        sound_event: sound.into(),
        position: Vec3 {
            x: position.0.x as f32,
            y: position.0.y as f32,
            z: position.0.z as f32,
        },
        data: -1,
        actor_identifier: kind.0.identifier.as_ref().into(),
        is_baby: is_baby(world, mob),
        is_global: false,
        actor_unique_id: world.get::<RuntimeId>(mob).map_or(0, |id| id.0),
        fire_at_position: None,
    };
    for viewer in &viewers.viewers {
        if let Some(session) = world.get::<PlayerSession>(*viewer) {
            let _ = session.send(McpePacket::from(packet.clone()));
        }
    }
}

/// Set or clear the in-love actor flag (the client draws hearts while it is
/// set) and refresh the flag words viewers receive.
pub(crate) fn set_love_flag(world: &mut World, mob: Entity, on: bool) {
    use jolyne::valentine::actor_data_i_ds as ids;
    let Some(mut flags) = world.get_mut::<ActorFlags>(mob) else {
        return;
    };
    flags
        .update(actor_flags::INLOVE, on)
        .expect("catalogue flag index");
    let words = flags.snapshot();
    let words = words.words();
    let (low, high) = (words[0] as i64, words[1] as i64);
    if let Some(mut metadata) = world.get_mut::<EntityMetadata>(mob) {
        metadata.set_long(ids::RESERVED_0, low);
        metadata.set_long(ids::RESERVED_092, high);
    }
}

#[cfg(test)]
#[path = "interaction_tests.rs"]
mod tests;
