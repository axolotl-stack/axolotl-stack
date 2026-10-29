//! Player interactions with mobs: dyeing, spawn eggs, feeding babies, love
//! mode and authored `minecraft:interact` rows (milking, shearing, saddling).
//!
//! [`player_interact`] tries, in order: a dye on a `minecraft:is_dyeable`
//! mob of another colour (recolours it), the mob's own spawn egg on a mob
//! with `minecraft:spawn_egg_interaction` (spawns a baby), `minecraft:ageable`
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

use crate::ecs::resources::TickCounter;
use crate::entity::EntityRandom;
use crate::entity::components::{
    ActorViewers, GameMode, HeldSlot, MainInventory, PlayerSession, Position, RuntimeId,
};
use crate::entity::definition::flags::ActorFlags;
use crate::entity::definition::metadata::EntityMetadata;
use crate::entity::mob::components::{
    AgeableConfig, BreedableConfig, InteractConfig, ItemMatcher, SpawnEggInteraction,
};
use crate::entity::mob::events::{MobFacts, held_item};
use crate::entity::mob::life::ageing::BabyAge;
use crate::entity::mob::state::{BreedCooldown, Dead, InLove, Persistent};
use crate::entity::mob::{MobKind, SpawnCause, constants, spawn_mob};
use crate::item::ItemStack;
use crate::server::game::types::ItemRegistryResource;
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
    let used = apply_dye(world, player, target, &held)
        || spawn_baby_from_egg(world, player, target, &held)
        || ageable_interaction(world, player, target, &held)
        || fall_in_love(world, player, target, &held)
        || authored_interaction(world, player, target);
    if used {
        world.entity_mut(target).insert(Persistent);
    }
}

/// `minecraft:is_dyeable`: a dye recolours the actor to the dye's colour and
/// one dye is used (kept in creative). A dye of the actor's own colour does
/// nothing.
fn apply_dye(world: &mut World, player: Entity, target: Entity, held: &ItemStack) -> bool {
    if world
        .get::<crate::entity::mob::components::color::DyeableConfig>(target)
        .is_none()
    {
        return false;
    }
    let Some(colour) = constants::DYE_ITEMS
        .iter()
        .position(|dye| *dye == held.item_id)
        .map(|index| index as u8)
    else {
        return false;
    };
    if crate::entity::mob::components::color::color_of(world, target) == Some(colour) {
        return false;
    }
    crate::entity::mob::components::color::set_color(
        &mut world.entity_mut(target),
        crate::entity::mob::components::color::ActorColor(colour),
    );
    use_held_item(world, player, None);
    true
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
/// stop it and reset items let it grow again. Every use is a feed (viewers
/// get [`feed_event`]); a feed item on a baby that is not paused also sends
/// `ActorEvent` BabyAge, which the client shows as happy-villager sparkles.
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
    feed_event(world, target, held);
    if fed && !matches!(age, BabyAge::Paused { .. }) {
        broadcast_actor_event(world, target, EnumsActorEvent::BabyAge, 0);
    }
    use_held_item(world, player, None);
    true
}

/// `minecraft:breedable`: a breed item puts an adult that is neither in
/// love nor cooling down into love mode for [`constants::LOVE_TICKS`]. The
/// item is fed ([`feed_event`]); the in-love flag makes the client draw
/// hearts.
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
    feed_event(world, target, held);
    use_held_item(world, player, None);
    true
}

/// Ticks after which `minecraft:interact` rows with a cooldown run again.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
#[component(storage = "SparseSet")]
pub struct InteractCooldown {
    pub until_tick: u64,
}

/// The first `minecraft:interact` row whose filter passes (other = the
/// player) and whose cooldown is not running: uses or wears the held item,
/// hands out `transform_to_item`, drops `spawn_items` loot, plays the row's
/// sounds, starts the cooldown and fires the row's event on the mob.
fn authored_interaction(world: &mut World, player: Entity, target: Entity) -> bool {
    let Some(config) = world.get::<InteractConfig>(target) else {
        return false;
    };
    let interactions = std::sync::Arc::clone(&config.interactions);
    let now = world.resource::<TickCounter>().current;
    let cooling = world
        .get::<InteractCooldown>(target)
        .is_some_and(|cooldown| now < cooldown.until_tick);
    let facts = MobFacts {
        world,
        entity: target,
        other: Some(player),
    };
    let Some(row) = interactions.iter().find(|row| {
        !(cooling && row.cooldown_ticks > 0)
            && row
                .filter
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
    if row.hurt_item > 0 {
        hurt_held_item(world, player, row.hurt_item);
    }
    if let Some(spawn) = &row.spawn_items {
        spawn_interaction_loot(world, target, spawn);
    }
    for sound in row.play_sounds.iter() {
        play_mob_sound(world, target, sound);
    }
    if row.cooldown_ticks > 0 {
        world.entity_mut(target).insert(InteractCooldown {
            until_tick: now + u64::from(row.cooldown_ticks),
        });
    }
    if let Some(event) = &row.on_interact {
        let handle = world
            .get::<MobKind>(target)
            .and_then(|kind| kind.0.events.event(&event.0));
        match handle {
            Some(handle) => {
                if let Err(error) =
                    crate::entity::mob::events::fire_event(world, target, &handle, Some(player))
                {
                    warn!(event = %event.0, %error, "interaction event failed");
                }
            }
            None => warn!(event = %event.0, "interaction event is not defined"),
        }
    }
    true
}

/// Wear the held item by `amount` durability: creative players and
/// unbreakable items take none, unbreaking level `n` spares it with
/// probability `n / (n + 1)`, and an item worn to its maximum breaks.
fn hurt_held_item(world: &mut World, player: Entity, amount: i32) {
    use crate::item::enchant::{
        Enchantment, durability_damage, enchantment_level, with_durability_damage,
    };
    if world.get::<GameMode>(player) == Some(&GameMode::Creative) {
        return;
    }
    let Some(held) = held_item(world, player).cloned() else {
        return;
    };
    let Some(max_damage) = crate::registry::item::tool_info(&held.item_id)
        .map(|info| info.max_damage)
        .filter(|max| *max > 0)
    else {
        return;
    };
    let unbreaking = enchantment_level(&held, Enchantment::Unbreaking);
    let roll = world
        .resource_mut::<EntityRandom>()
        .0
        .gen_range(0..=unbreaking);
    // Level `n` applies wear with probability `1 / (n + 1)`.
    if unbreaking > 0 && roll > 0 {
        return;
    }
    let damage = durability_damage(&held) + amount;
    let worn = if damage >= max_damage {
        ItemStack::empty()
    } else {
        with_durability_damage(&held, damage)
    };
    let slot = world.get::<HeldSlot>(player).map_or(0, |slot| slot.0);
    if let Some(mut inventory) = world.get_mut::<MainInventory>(player)
        && let Some(stack) = inventory.hotbar_mut(slot)
    {
        *stack = worn.clone();
    }
    send_slot(world, player, usize::from(slot), &worn);
}

/// Roll an interaction's `spawn_items` table for `mob` (its colour and fire
/// state are the THIS actor's) and drop the items at its feet raised by the
/// row's offset.
fn spawn_interaction_loot(
    world: &mut World,
    mob: Entity,
    spawn: &crate::entity::mob::components::SpawnItems,
) {
    use rand::SeedableRng;
    let Some(table) = unastar_data::loot::get(&spawn.table) else {
        warn!(table = %spawn.table, "interaction loot table is not in the loot registry");
        return;
    };
    let Some(feet) = world.get::<Position>(mob).map(|p| p.0) else {
        return;
    };
    let recipes = world
        .get_resource::<crate::server::game::types::RecipeRegistryResource>()
        .map(|r| r.0.clone());
    let ctx = crate::server::game::LootContext {
        on_fire: Some(
            world
                .get::<crate::entity::components::OnFire>(mob)
                .is_some_and(crate::entity::components::OnFire::is_on_fire),
        ),
        recipes: recipes.as_deref(),
        color_index: crate::entity::mob::components::color::color_of(world, mob),
        ..Default::default()
    };
    let seed = world.resource_mut::<EntityRandom>().0.r#gen::<u64>();
    let drops = match unastar_rules::loot::roll(
        unastar_data::loot::vanilla(),
        table,
        &ctx,
        &mut rand::rngs::StdRng::seed_from_u64(seed),
    ) {
        Ok(drops) => drops,
        Err(error) => {
            warn!(table = %spawn.table, %error, "interaction loot table cannot be evaluated");
            return;
        }
    };
    let at = feet + DVec3::new(0.0, f64::from(spawn.y_offset), 0.0);
    for stack in crate::server::game::loot_stacks(world, drops) {
        let Some(item) = crate::entity::spawn_item_drop(world, at, stack, None) else {
            continue;
        };
        let jitter = {
            let rng = &mut world.resource_mut::<EntityRandom>().0;
            DVec3::new(
                (rng.r#gen::<f64>() - rng.r#gen::<f64>()) * constants::INTERACT_SPAWN_ITEM_SPREAD,
                rng.r#gen::<f64>() * constants::INTERACT_SPAWN_ITEM_LIFT,
                (rng.r#gen::<f64>() - rng.r#gen::<f64>()) * constants::INTERACT_SPAWN_ITEM_SPREAD,
            )
        };
        if let Some(mut velocity) = world.get_mut::<crate::entity::components::Velocity>(item) {
            velocity.0 += jitter;
        }
    }
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

/// Viewers see `mob` eat `held`: `ActorEvent` Feed whose data is the item's
/// network id above [`constants::FEED_EVENT_ITEM_SHIFT`] and its aux value
/// below. The client spawns the item's crumbs at the mob's mouth and plays
/// its eating sound.
fn feed_event(world: &World, mob: Entity, held: &ItemStack) {
    let Some(network_id) = world
        .resource::<ItemRegistryResource>()
        .0
        .get_by_name(&held.item_id)
        .map(|item| item.network_id)
    else {
        return;
    };
    let data = (network_id << constants::FEED_EVENT_ITEM_SHIFT) | i32::from(held.damage as u16);
    broadcast_actor_event(world, mob, EnumsActorEvent::Feed, data);
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
