//! `minecraft:damage_sensor`: per-cause damage rules evaluated before a mob
//! is hurt.
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

use std::sync::Arc;

use bevy_ecs::prelude::*;
use thiserror::Error;
use tracing::warn;
use unastar_data::actor_components::body::{
    self, ActorDamageCause, ActorDefinitionTrigger, DealsDamage, DealsDamageMode, FilterSubject,
};
use unastar_rules::filter::Filter;

use super::MobKind;
use super::combat::Hit;
use super::events::{MobFacts, fire_event};
use crate::entity::definition::ComponentKindsBuilder;

/// Why a `minecraft:damage_sensor` body cannot be compiled.
#[derive(Debug, Error)]
pub enum DamageSensorError {
    #[error("component-level deals_damage is not implemented")]
    ComponentDealsDamage,
    #[error("deals_damage no_but_entity_effects_apply is not implemented")]
    EntityEffectsMode,
    #[error("on_damage event {0} targets another actor; only self is implemented")]
    OtherTarget(String),
    #[error("a trigger sets both on_damage and the legacy event/filters fields")]
    MixedTrigger,
    #[error("filter: {0}")]
    Filter(String),
}

/// What a matching trigger does to the damage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DealsDamageRule {
    Yes,
    No,
    /// Zero damage, but the hurt effects (animation, knockback) still apply.
    NoButSideEffects,
}

/// One compiled trigger.
#[derive(Debug, Clone)]
pub struct SensorTrigger {
    /// `None` matches every cause (authored `all` or unset).
    pub cause: Option<ActorDamageCause>,
    pub filter: Option<Filter>,
    /// Event fired on the mob itself when the trigger matches.
    pub event: Option<Arc<str>>,
    /// Level sound played at the mob's mid-body when the trigger matches.
    pub sound: Option<Arc<str>>,
    pub deals: DealsDamageRule,
    pub damage_modifier: f32,
    pub damage_multiplier: f32,
}

/// `minecraft:damage_sensor`.
#[derive(Component, Debug, Clone)]
pub struct DamageSensorConfig {
    pub triggers: Arc<[SensorTrigger]>,
}

pub fn register_component_kinds(builder: &mut ComponentKindsBuilder) -> Result<(), String> {
    super::config::insert(builder, |b: body::DamageSensor| {
        compile(b).map_err(|e| e.to_string())
    })
}

fn compile(b: body::DamageSensor) -> Result<DamageSensorConfig, DamageSensorError> {
    if b.deals_damage.is_some() {
        return Err(DamageSensorError::ComponentDealsDamage);
    }
    let triggers = b
        .triggers
        .unwrap_or_default()
        .into_iter()
        .map(compile_trigger)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(DamageSensorConfig {
        triggers: triggers.into(),
    })
}

fn compile_trigger(t: body::DamageSensorTrigger) -> Result<SensorTrigger, DamageSensorError> {
    let deals = match t.deals_damage {
        DealsDamage::Bool(true) | DealsDamage::Mode(DealsDamageMode::Yes) => DealsDamageRule::Yes,
        DealsDamage::Bool(false) | DealsDamage::Mode(DealsDamageMode::No) => DealsDamageRule::No,
        DealsDamage::Mode(DealsDamageMode::NoButSideEffectsApply) => {
            DealsDamageRule::NoButSideEffects
        }
        DealsDamage::Mode(DealsDamageMode::NoButEntityEffectsApply) => {
            return Err(DamageSensorError::EntityEffectsMode);
        }
    };
    // `on_damage` carries the filters and event; older content writes them
    // on the trigger itself.
    let (filters, event) = match t.on_damage {
        Some(ActorDefinitionTrigger {
            event,
            filters,
            target,
        }) => {
            if t.event.is_some() || t.filters.is_some() {
                return Err(DamageSensorError::MixedTrigger);
            }
            if target != FilterSubject::SelfValue && !event.is_empty() {
                return Err(DamageSensorError::OtherTarget(event));
            }
            (filters, (!event.is_empty()).then_some(event))
        }
        None => (t.filters, t.event.filter(|event| !event.is_empty())),
    };
    let filter = filters
        .as_ref()
        .map(super::config::filter)
        .transpose()
        .map_err(DamageSensorError::Filter)?;
    Ok(SensorTrigger {
        cause: (!matches!(t.cause, ActorDamageCause::All | ActorDamageCause::None))
            .then_some(t.cause),
        filter,
        event: event.map(|event| Arc::from(event.as_str())),
        sound: t
            .on_damage_sound_event
            .filter(|sound| !sound.is_empty())
            .map(|sound| Arc::from(sound.as_str())),
        deals,
        damage_modifier: t.damage_modifier,
        damage_multiplier: t.damage_multiplier,
    })
}

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
        && let Some(centre) = super::sound::body_centre(world, target)
    {
        super::sound::play_level_sound(world, Some(target), centre, sound, -1);
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
