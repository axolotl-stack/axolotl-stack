//! `minecraft:damage_sensor`: per-cause damage rules evaluated before a mob
//! is hurt (the component is compiled in `mob::components::damage_sensor`).
//!
//! Native rule: the triggers are scanned in authored order and the first one
//! whose cause matches (the damage's own cause, `all`, or unset) and whose
//! filters pass (`other` is the attacker) decides alone. Self-destruct damage
//! bypasses the sensor. A matching trigger adjusts the damage
//! (`(damage - damage_modifier) * damage_multiplier`); with
//! `deals_damage: no` the hit is dropped entirely (no health change, hurt
//! animation, sound or knockback); `no_but_side_effects_apply` keeps the
//! hurt effects with zero damage. For a trigger that cancels damage the
//! `on_damage` event and its sound run before the cancel; for one that deals
//! damage they run after the damage applied.

use bevy_ecs::prelude::*;
use tracing::warn;
use unastar_data::actor_components::body::ActorDamageCause;

use super::Hit;
use crate::entity::mob::MobKind;
use crate::entity::mob::components::{DamageSensorConfig, DealsDamageRule, SensorTrigger};
use crate::entity::mob::events::{MobFacts, fire_event};

/// The sensor's verdict on one hit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Sensed {
    /// Apply this hit; fire `after` (the matched trigger) once it applied.
    Hurt { hit: Hit, after: Option<usize> },
    /// Drop the hit.
    Cancelled,
}

/// Run `target`'s damage sensor on `hit`.
pub fn sense(world: &mut World, target: Entity, hit: Hit) -> Sensed {
    if hit.cause == ActorDamageCause::SelfDestruct {
        return Sensed::Hurt { hit, after: None };
    }
    let Some(config) = world.get::<DamageSensorConfig>(target).cloned() else {
        return Sensed::Hurt { hit, after: None };
    };
    let Some(index) = config.triggers.iter().position(|trigger| {
        trigger.cause.is_none_or(|cause| cause == hit.cause)
            && trigger.filter.as_ref().is_none_or(|filter| {
                filter.evaluate(&MobFacts {
                    world,
                    entity: target,
                    other: hit.attacker,
                })
            })
    }) else {
        return Sensed::Hurt { hit, after: None };
    };
    let trigger = &config.triggers[index];
    let damage = (hit.damage - trigger.damage_modifier) * trigger.damage_multiplier;
    match trigger.deals {
        DealsDamageRule::Yes => Sensed::Hurt {
            hit: Hit { damage, ..hit },
            after: Some(index),
        },
        DealsDamageRule::No => {
            run_trigger(world, target, trigger, hit.attacker);
            Sensed::Cancelled
        }
        DealsDamageRule::NoButSideEffects => {
            run_trigger(world, target, trigger, hit.attacker);
            Sensed::Hurt {
                hit: Hit { damage: 0.0, ..hit },
                after: None,
            }
        }
    }
}

/// Fire the `on_damage` event and sound of trigger `index` after a hit that
/// dealt damage.
pub fn after_hurt(world: &mut World, target: Entity, index: usize, attacker: Option<Entity>) {
    let Some(config) = world.get::<DamageSensorConfig>(target).cloned() else {
        return;
    };
    if let Some(trigger) = config.triggers.get(index) {
        run_trigger(world, target, trigger, attacker);
    }
}

fn run_trigger(
    world: &mut World,
    target: Entity,
    trigger: &SensorTrigger,
    attacker: Option<Entity>,
) {
    if let Some(sound) = &trigger.sound
        && let Some(centre) = crate::entity::mob::net::sound::body_centre(world, target)
    {
        crate::entity::mob::net::sound::play_level_sound(world, Some(target), centre, sound, -1);
    }
    let Some(name) = &trigger.event else {
        return;
    };
    let Some(handle) = world
        .get::<MobKind>(target)
        .and_then(|kind| kind.0.events.event(name))
    else {
        warn!(event = %name, "damage_sensor names an event the mob does not define");
        return;
    };
    if let Err(error) = fire_event(world, target, &handle, attacker) {
        warn!(event = %name, %error, "damage_sensor event failed");
    }
}
