//! Hurting mobs: player melee, `minecraft:hurt_on_condition`, the hurt
//! timer and knockback. Death (loot, experience, removal) is in
//! [`super::death`]; the knockback rules component is
//! `mob::components::knockback`.
//!
//! Every damage source goes through [`hurt_mob`], which applies the native
//! hurt rule: a successful hit starts a [`constants::HURT_TICKS`] hurt
//! timer; while it runs a new hit only deals what exceeds the damage that
//! started it, and nothing when it does not exceed it (override and
//! self-destruct damage ignore the timer). A full hit shows the hurt
//! animation to viewers (`ActorEvent` hurt) and, when it has a source
//! position, knocks the mob back. The changed `Health` reaches viewers as
//! an `UpdateAttributes` through `derive_mob_state`.

use std::sync::Arc;

use bevy_ecs::prelude::*;
use glam::{DVec2, DVec3};
use jolyne::valentine::types::{
    ActorRuntimeId, EnumsActorEvent, ItemUseOnActorInventoryTransaction,
};
use jolyne::valentine::{ActorEventPacket, McpePacket};
use rand::Rng;
use unastar_data::actor_components::body::ActorDamageCause;

use super::damage_sensor::{self, Sensed};
use super::death;
use crate::ecs::resources::TickCounter;
use crate::entity::EntityRandom;
use crate::entity::components::{
    ActorViewers, GameMode, Health, PhysicsBody, Player, PlayerSession, Position, RuntimeId,
    Velocity,
};
use crate::entity::mob::components::{
    HurtCondition, HurtOnConditionConfig, KnockbackPower, KnockbackRules,
};
use crate::entity::mob::events::{MobFacts, held_item};
use crate::entity::mob::state::{Dead, LastHurt};
use crate::entity::mob::{MobKind, constants};
use crate::registry::collision::Aabb;

/// The hurt timer of a mob: the server tick it runs out at and the damage
/// of the hit that started it.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
#[component(storage = "SparseSet")]
pub struct HurtTimer {
    pub until_tick: u64,
    pub last_damage: f32,
}

/// One instance of damage dealt to a mob.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hit {
    pub damage: f32,
    pub cause: ActorDamageCause,
    pub attacker: Option<Entity>,
    pub by_player: bool,
    /// Position the knockback pushes away from (horizontally); `None` for
    /// damage without knockback.
    pub knockback_from: Option<DVec3>,
    /// Strength of that knockback (the attacker's knockback rules).
    pub knockback_power: KnockbackPower,
}

/// The knockback `attacker` deals: its knockback rules, else the built-in
/// preset.
pub fn knockback_power_of(world: &World, attacker: Entity) -> KnockbackPower {
    world
        .get::<KnockbackRules>(attacker)
        .map_or(KnockbackPower::DEFAULT, |rules| rules.0)
}

/// A player attacks `target` with whatever the server has in the player's
/// selected hotbar slot; the item and positions the client reports in the
/// transaction are not trusted.
///
/// Spectators, dead players and players whose eye is farther than
/// [`constants::MELEE_REACH`] from the mob's box cannot hit. Creative
/// players deal normal melee damage.
pub fn player_attack(
    world: &mut World,
    player: Entity,
    target: Entity,
    _transaction: &ItemUseOnActorInventoryTransaction,
) {
    let Some(eye) = attacker_eye(world, player) else {
        return;
    };
    let (Some(feet), Some(body)) = (
        world.get::<Position>(target).map(|p| p.0),
        world.get::<PhysicsBody>(target),
    ) else {
        return;
    };
    let mob_box = Aabb::from_feet(feet, body.width, body.height);
    if eye.distance_squared(eye.clamp(mob_box.min, mob_box.max))
        > constants::MELEE_REACH * constants::MELEE_REACH
    {
        return;
    }
    let held = held_item(world, player).cloned();
    let damage = unastar_data::melee::attack_damage(held.as_ref().map(|stack| stack.item_id()));
    let applied = hurt_mob(
        world,
        target,
        Hit {
            damage,
            cause: ActorDamageCause::EntityAttack,
            attacker: Some(player),
            by_player: true,
            knockback_from: Some(eye),
            knockback_power: KnockbackPower::DEFAULT,
        },
    );
    // The attacker pays food and, for a hit that landed, weapon durability.
    crate::server::game::hunger::exhaust_player(world, player, |values| values.attack);
    if applied > 0.0
        && let Some(entry) = held.as_ref().and_then(|stack| stack.item())
    {
        let wear = crate::server::game::durability::attack_wear(entry);
        crate::server::game::durability::wear_held(world, player, wear);
    }
}

/// The eye position of a player that may attack this tick.
fn attacker_eye(world: &World, player: Entity) -> Option<DVec3> {
    world.get::<Player>(player)?;
    if *world.get::<GameMode>(player)? == GameMode::Spectator
        || world.get::<Health>(player).is_some_and(Health::is_dead)
    {
        return None;
    }
    let eye = world.get::<Position>(player)?.0;
    eye.is_finite().then_some(eye)
}

/// Apply `hit` to `target` under the damage sensor and the hurt rules;
/// returns the health actually removed (0 when the hit was ignored). A hit
/// that empties the mob's health starts its death. Zero damage still counts
/// as a hit (hurt animation, hurt timer, knockback), as BDS hurts with it.
pub fn hurt_mob(world: &mut World, target: Entity, hit: Hit) -> f32 {
    if hit.damage < 0.0 || world.get::<Dead>(target).is_some() {
        return 0.0;
    }
    if world.get::<Health>(target).is_none_or(Health::is_dead) {
        return 0.0;
    }
    if crate::entity::conditions::rejects_fire_damage(world, target, hit.cause) {
        return 0.0;
    }
    let (hit, sensor_trigger) = match damage_sensor::sense(world, target, hit) {
        Sensed::Hurt { hit, after } => (hit, after),
        Sensed::Cancelled => return 0.0,
    };
    if hit.damage < 0.0 {
        return 0.0;
    }
    let ignores_timer = matches!(
        hit.cause,
        ActorDamageCause::Override | ActorDamageCause::SelfDestruct
    );
    let now = world.resource::<TickCounter>().current;
    let timer = world.get::<HurtTimer>(target).copied();
    let (applied, until_tick, full) = match timer {
        Some(timer) if now < timer.until_tick && !ignores_timer => {
            if hit.damage <= timer.last_damage {
                return 0.0;
            }
            (hit.damage - timer.last_damage, timer.until_tick, false)
        }
        _ => (hit.damage, now + u64::from(constants::HURT_TICKS), true),
    };
    let dead = {
        let mut entity = world.entity_mut(target);
        entity.insert((
            HurtTimer {
                until_tick,
                last_damage: hit.damage,
            },
            LastHurt {
                tick: now,
                cause: hit.cause,
                attacker: hit.attacker,
                by_player: hit.by_player,
            },
        ));
        let mut health = entity.get_mut::<Health>().expect("health checked above");
        health.damage(applied);
        health.is_dead()
    };
    if full {
        send_actor_event(world, target, EnumsActorEvent::Hurt, 0);
        if let Some(from) = hit.knockback_from {
            apply_knockback(world, target, from, hit.knockback_power);
        }
    }
    if let Some(index) = sensor_trigger {
        damage_sensor::after_hurt(world, target, index, hit.attacker);
    }
    crate::entity::mob::life::triggers::hurt(world, target, hit.attacker, hit.by_player);
    // A hurt player's armour wears and the player pays food for it.
    if world.get::<Player>(target).is_some() && applied > 0.0 {
        crate::server::game::durability::wear_armour(world, target, hit.damage, hit.cause);
        if crate::server::game::durability::armour_wears_from(hit.cause) {
            crate::server::game::hunger::exhaust_player(world, target, |values| values.damage);
        }
    }
    if dead {
        death::begin_death(world, target);
    } else if hit.cause == ActorDamageCause::Drowning {
        crate::entity::mob::teleport::escape_water(world, target);
    }
    applied
}

/// Push `target` away from `from` (horizontally).
fn apply_knockback(world: &mut World, target: Entity, from: DVec3, power: KnockbackPower) {
    let Some(feet) = world.get::<Position>(target).map(|p| p.0) else {
        return;
    };
    let mut away = DVec2::new(feet.x - from.x, feet.z - from.z);
    if away.length_squared() < 1.0e-8 {
        // Attacker straight above or below: a random horizontal direction.
        let angle = world
            .get_resource_or_init::<EntityRandom>()
            .0
            .gen_range(0.0..std::f64::consts::TAU);
        away = DVec2::from_angle(angle);
    }
    if let Some(mut velocity) = world.get_mut::<Velocity>(target) {
        velocity.0 = knockback_velocity(velocity.0, away.normalize(), power);
    }
}

/// Native knockback: existing velocity × `power.retain`, plus
/// [`constants::KNOCKBACK_HORIZONTAL`] × `power.horizontal` along `away` (a
/// unit vector in x/z) and `power.vertical × KNOCKBACK_VERTICAL_SCALE`
/// upwards, the vertical velocity capped at `power.vertical_cap`.
pub fn knockback_velocity(velocity: DVec3, away: DVec2, power: KnockbackPower) -> DVec3 {
    let mut out = velocity * power.retain;
    out.x += away.x * constants::KNOCKBACK_HORIZONTAL * power.horizontal;
    out.z += away.y * constants::KNOCKBACK_HORIZONTAL * power.horizontal;
    out.y = (out.y + power.vertical * constants::KNOCKBACK_VERTICAL_SCALE).min(power.vertical_cap);
    out
}

/// Send `ActorEvent(event, data)` about `entity` to its viewers.
pub fn send_actor_event(world: &World, entity: Entity, event: EnumsActorEvent, data: i32) {
    let (Some(viewers), Some(runtime_id)) = (
        world.get::<ActorViewers>(entity),
        world.get::<RuntimeId>(entity),
    ) else {
        return;
    };
    let packet = McpePacket::from(ActorEventPacket {
        target_runtime_id: ActorRuntimeId {
            actor_runtime_id: runtime_id.0 as u64,
        },
        event_id: event,
        data,
        fire_at_position: None,
    });
    for viewer in &viewers.viewers {
        if let Some(session) = world.get::<PlayerSession>(*viewer) {
            let _ = session.send(packet.clone());
        }
    }
}

/// System (exclusive): `minecraft:hurt_on_condition`. Every tick each
/// condition whose filter passes deals its `damage_per_tick` with its cause,
/// under the hurt rules and without knockback.
#[allow(clippy::type_complexity)]
pub fn hurt_on_condition(
    world: &mut World,
    mobs: &mut QueryState<(Entity, &HurtOnConditionConfig), (With<MobKind>, Without<Dead>)>,
    mut due: Local<Vec<(Entity, Arc<[HurtCondition]>)>>,
) {
    due.extend(
        mobs.iter(world)
            .map(|(entity, config)| (entity, Arc::clone(&config.conditions))),
    );
    for (entity, conditions) in due.drain(..) {
        for condition in conditions.iter() {
            if world.get::<Dead>(entity).is_some() {
                break;
            }
            let passes = condition.filter.evaluate(&MobFacts {
                world,
                entity,
                other: None,
            });
            if passes {
                hurt_mob(
                    world,
                    entity,
                    Hit {
                        damage: condition.damage_per_tick as f32,
                        cause: condition.cause.unwrap_or(ActorDamageCause::None),
                        attacker: None,
                        by_player: false,
                        knockback_from: None,
                        knockback_power: KnockbackPower::DEFAULT,
                    },
                );
            }
        }
    }
}

#[cfg(test)]
mod tests;
