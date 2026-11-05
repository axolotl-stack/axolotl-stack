//! Leads: leashing a mob to a player, pulling it along and breaking.
//!
//! A lead used on a mob with `minecraft:leashable` that is not on a lead
//! (or whose lead `can_be_stolen`) leashes it to the player: the mob's
//! actor data `LEASH_HOLDER` is the player's unique id and its `LEASHED`
//! flag is set (clients draw the lead), `lead.leash` plays at the mob, its
//! `on_leash` event fires, one lead is used up outside creative and the mob
//! becomes persistent. Interacting with a mob one holds on a lead unleashes
//! it and drops the lead. Unleashed, `LEASH_HOLDER` is -1.
//!
//! Every tick ([`tick_leashes`]) a leashed mob farther than its
//! `max_distance` from the holder's feet (10 blocks when the definition
//! leaves it at 0) breaks free: `ActorEvent` 63 (data 1) on the mob, the
//! lead drops at the mob and `lead.break` plays at the holder. Farther than
//! its `hard_distance` it is pulled: each axis of the unit offset towards
//! the holder adds `0.4 × axis²` (signed) to its velocity. A mob whose
//! holder left, died or became a spectator, or that died itself, drops the
//! lead.
//!
//! Measured on BDS 1.26.51 (client-diff `lead-nametag`, items3 capture):
//! the leash actor data and flag, no actor link, the leash and break sounds,
//! the break event and the dropped lead. Public reference, unmeasured
//! (recorded in `docs/quirks/bds.md`): the pull and the break distance default.
//! Not implemented: walking towards the holder between `soft_distance` and
//! `hard_distance` (needs a mob AI goal), tying leads to fences (BDS spawns
//! a `minecraft:leash_knot` actor that becomes the holder; it has no
//! definition or spawn path here), mob-to-mob leads and saving the lead
//! with the mob.

use bevy_ecs::prelude::*;
use glam::DVec3;
use jolyne::valentine::{actor_data_i_ds as ids, actor_flags};
use tracing::warn;

use crate::entity::PLAYER_EYE_HEIGHT;
use crate::entity::components::{GameMode, Health, Player, Position, Velocity};
use crate::entity::definition::flags::ActorFlags;
use crate::entity::definition::metadata::EntityMetadata;
use crate::entity::item_drop::drop_thrower_id;
use crate::entity::mob::MobKind;
use crate::entity::mob::components::SelfEvent;
use crate::entity::mob::components::leash::LeashableConfig;
use crate::entity::mob::net::sound::play_level_sound;
use crate::entity::mob::state::{Dead, Leashed};
use crate::item::ItemStack;
use crate::server::game::world_stack;

pub const LEAD: &str = "minecraft:lead";
/// Velocity a pulled mob gains per squared unit-offset axis.
const PULL: f64 = 0.4;

/// A player interacting with a mob it holds on a lead unleashes it and the
/// lead drops. Whether it did.
pub(super) fn release(world: &mut World, player: Entity, target: Entity) -> bool {
    if world.get::<Leashed>(target).map(|leash| leash.holder) != Some(player) {
        return false;
    }
    unleash(world, target, true);
    true
}

/// A lead used on a leashable mob leashes it to the player. Whether it did.
pub(super) fn use_lead(
    world: &mut World,
    player: Entity,
    target: Entity,
    held: &ItemStack,
) -> bool {
    if held.item_id() != LEAD {
        return false;
    }
    let holder = world.get::<Leashed>(target).map(|leash| leash.holder);
    let Some(config) = world.get::<LeashableConfig>(target).cloned() else {
        return false;
    };
    if holder.is_some() && !config.can_be_stolen {
        return false;
    }
    let Some(holder_id) = drop_thrower_id(world, player) else {
        return false;
    };
    set_leash_state(world, target, holder_id, true);
    if let Some(position) = world.get::<Position>(target).map(|p| p.0) {
        play_level_sound(world, Some(target), position, "lead.leash", -1);
    }
    world.entity_mut(target).insert(Leashed { holder: player });
    super::interact::use_held_item(world, player, None);
    if let Some(event) = &config.on_leash {
        fire(world, target, event);
    }
    true
}

/// Take `mob` off its lead, dropping the lead item at the mob when `drop`.
pub fn unleash(world: &mut World, mob: Entity, drop: bool) {
    if world.entity_mut(mob).take::<Leashed>().is_none() {
        return;
    }
    set_leash_state(world, mob, -1, false);
    if let Some(event) = world
        .get::<LeashableConfig>(mob)
        .and_then(|config| config.on_unleash.clone())
    {
        fire(world, mob, &event);
    }
    if drop
        && let Some(position) = world.get::<Position>(mob).map(|p| p.0)
        && let Some(lead) = world_stack(world, LEAD, 1)
    {
        crate::entity::spawn_item_drop(world, position, lead, None);
    }
}

/// Actor data `LEASH_HOLDER` and the `LEASHED` flag.
fn set_leash_state(world: &mut World, mob: Entity, holder: i64, leashed: bool) {
    let mut entity = world.entity_mut(mob);
    let words = entity.get_mut::<ActorFlags>().map(|mut flags| {
        flags
            .update(actor_flags::LEASHED, leashed)
            .expect("catalogue flag index");
        *flags.snapshot().words()
    });
    if let Some(mut metadata) = entity.get_mut::<EntityMetadata>() {
        metadata.set_long(ids::LEASH_HOLDER, holder);
        if let Some(words) = words {
            metadata.set_long(ids::RESERVED_0, words[0] as i64);
            metadata.set_long(ids::RESERVED_092, words[1] as i64);
        }
    }
}

fn fire(world: &mut World, mob: Entity, event: &SelfEvent) {
    let handle = world
        .get::<MobKind>(mob)
        .and_then(|kind| kind.0.events.event(&event.0));
    match handle {
        Some(handle) => {
            if let Err(error) = crate::entity::mob::events::fire_event(world, mob, &handle, None) {
                warn!(event = %event.0, %error, "leash event failed");
            }
        }
        None => warn!(event = %event.0, "leash event is not defined"),
    }
}

/// The holder's feet, if it can still hold a lead.
fn holder_feet(world: &World, holder: Entity) -> Option<DVec3> {
    if world.get::<Player>(holder).is_none()
        || world.get::<Health>(holder).is_some_and(Health::is_dead)
        || world.get::<GameMode>(holder) == Some(&GameMode::Spectator)
    {
        return None;
    }
    Some(world.get::<Position>(holder)?.0 - DVec3::Y * PLAYER_EYE_HEIGHT)
}

/// The velocity a lead adds to a mob `offset` away from its holder
/// (`distance` = its length).
fn pull(offset: DVec3, distance: f64) -> DVec3 {
    let unit = offset / distance;
    DVec3::new(
        (unit.x * unit.x * PULL).copysign(unit.x),
        (unit.y * unit.y * PULL).copysign(unit.y),
        (unit.z * unit.z * PULL).copysign(unit.z),
    )
}

/// System (exclusive, `PhysicsSet`, before the physics step): pull leashed
/// mobs towards their holders and break leads that are stretched too far or
/// lost their holder.
pub fn tick_leashes(
    world: &mut World,
    leashed: &mut QueryState<(Entity, &Leashed)>,
    mut due: Local<Vec<(Entity, Entity)>>,
) {
    due.extend(leashed.iter(world).map(|(mob, leash)| (mob, leash.holder)));
    for (mob, holder) in due.drain(..) {
        let held = holder_feet(world, holder)
            .zip(
                world
                    .get::<LeashableConfig>(mob)
                    .map(|config| (config.hard_distance, config.max_distance)),
            )
            .filter(|_| world.get::<Dead>(mob).is_none());
        let Some((feet, (hard_distance, max_distance))) = held else {
            unleash(world, mob, true);
            continue;
        };
        let Some(position) = world.get::<Position>(mob).map(|p| p.0) else {
            continue;
        };
        let offset = feet - position;
        let distance = offset.length();
        if distance > max_distance {
            crate::entity::mob::combat::send_actor_event(
                world,
                mob,
                jolyne::valentine::types::EnumsActorEvent::LeashDestroyed,
                1,
            );
            unleash(world, mob, true);
            play_level_sound(world, Some(holder), feet, "lead.break", -1);
        } else if distance > hard_distance
            && let Some(mut velocity) = world.get_mut::<Velocity>(mob)
        {
            velocity.0 += pull(offset, distance);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pull_grows_with_the_squared_unit_offset() {
        let pulled = pull(DVec3::new(-8.0, 0.0, 6.0), 10.0);
        assert!((pulled.x + 0.64 * PULL).abs() < 1e-12);
        assert_eq!(pulled.y, 0.0);
        assert!((pulled.z - 0.36 * PULL).abs() < 1e-12);
    }
}
