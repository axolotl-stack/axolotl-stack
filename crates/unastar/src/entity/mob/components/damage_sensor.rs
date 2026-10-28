//! `minecraft:damage_sensor`: per-cause damage rules evaluated before a mob
//! is hurt. This file compiles the component; the sensor runs in
//! `mob::combat::damage_sensor`.
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
use unastar_data::actor_components::body::{
    self, ActorDamageCause, ActorDefinitionTrigger, DealsDamage, DealsDamageMode, FilterSubject,
};
use unastar_rules::filter::Filter;

use super::common::{FilterError, filter, insert};
use crate::entity::definition::{ComponentError, ComponentKindsBuilder, DefinitionError};

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
    Filter(#[from] FilterError),
}

impl ComponentError for DamageSensorError {}

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

/// Register the `minecraft:damage_sensor` component kind.
pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
) -> Result<(), DefinitionError> {
    insert(builder, compile)
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
    let filter = filters.as_ref().map(filter).transpose()?;
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
