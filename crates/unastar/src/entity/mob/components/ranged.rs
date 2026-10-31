//! Authored ranged attacks, projectile choices and target-distance events.

use std::sync::Arc;

use bevy_ecs::prelude::*;
use thiserror::Error;
use unastar_data::actor_components::body::{self, EventTrigger, FilterSubject};
use unastar_data::actor_identifiers::ActorType;
use unastar_rules::filter::Filter;

use super::common::{FilterError, filter, insert};
use super::hostile::controls;
use super::triggers::{ActorTrigger, TriggerError};
use crate::entity::definition::{
    ComponentError, ComponentKindsBuilder, DefinitionError, DefinitionState,
};
use crate::entity::goal_selector::GoalControls;
use crate::entity::mob::events::MobFacts;

/// Invalid configuration and deliberately unsupported ranged options.
#[derive(Debug, Error)]
pub enum RangedError {
    #[error("{0} requires finite, ordered nonnegative bounds")]
    Bounds(&'static str),
    #[error("ranged option {0} is not implemented")]
    Unsupported(&'static str),
    #[error("projectile definition {0} is not a supported native actor identifier")]
    Identifier(String),
    #[error("shooter requires at least one projectile")]
    EmptyShooter,
    #[error("shooter projectile {0} is not spawnable")]
    Unspawnable(&'static str),
    #[error("shooter projectile {0} has no base projectile behavior")]
    NotProjectile(&'static str),
    #[error(transparent)]
    Projectile(#[from] crate::entity::projectile::ProjectileError),
    #[error(transparent)]
    Filter(#[from] FilterError),
    #[error(transparent)]
    Trigger(#[from] TriggerError),
    #[error("shooter body: {0}")]
    Decode(String),
}
impl ComponentError for RangedError {}

/// A conditional authored projectile choice.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectileChoice {
    pub actor: ActorType,
    pub filter: Option<Filter>,
}

/// `minecraft:shooter`: immutable choices and actual launch customization.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct ShooterConfig {
    pub default: Option<ActorType>,
    pub choices: Arc<[ProjectileChoice]>,
    pub power: Option<f64>,
    pub sound: Option<Arc<str>>,
}

impl ShooterConfig {
    /// Resolve conditions against the current shooter and target, in authored order.
    pub(crate) fn choose(&self, world: &World, actor: Entity, target: Entity) -> Option<ActorType> {
        self.choices
            .iter()
            .find(|choice| {
                choice.filter.as_ref().is_none_or(|filter| {
                    filter.evaluate(&MobFacts {
                        world,
                        entity: actor,
                        other: Some(target),
                    })
                })
            })
            .map(|choice| choice.actor)
            .or(self.default)
    }

    /// All possible definitions, for startup capability validation.
    pub(crate) fn projectiles(&self) -> impl Iterator<Item = ActorType> + '_ {
        self.choices
            .iter()
            .map(|choice| choice.actor)
            .chain(self.default)
    }
}

/// `minecraft:behavior.ranged_attack`, with seconds converted once at compilation.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct RangedAttackGoal {
    pub priority: i32,
    pub(crate) controls: GoalControls,
    pub attack_interval: (u64, u64),
    pub min_range: f64,
    pub max_range: f64,
    pub burst_interval: u64,
    pub burst_shots: u16,
    pub charge_ticks: i64,
    pub fov: f32,
    pub speed_multiplier: f32,
    pub sight_ticks: u64,
    pub movement_mode: body::RangedAttackGoalDefinitionInRangeMovementMode,
    pub x_max_rotation: f32,
    pub y_max_rotation: f32,
    pub set_persistent: bool,
}

/// Current target boundary and optional visibility transitions.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct TargetNearbySensor {
    pub inside_range: f64,
    pub outside_range: f64,
    pub must_see: bool,
    pub inside: Option<ActorTrigger>,
    pub outside: Option<ActorTrigger>,
    pub vision_lost: Option<ActorTrigger>,
}

/// Transitions belong to the composed sensor, rather than a global target cache.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq)]
#[component(storage = "SparseSet")]
pub(crate) struct TargetNearbyState {
    pub target: Option<Entity>,
    pub inside: Option<bool>,
    pub visible: bool,
}

/// Validate and convert a bounded authored duration without saturating bad input.
fn ticks(value: f32, name: &'static str) -> Result<u64, RangedError> {
    if !value.is_finite() || !(0.0..=86_400.0).contains(&value) {
        return Err(RangedError::Bounds(name));
    }
    Ok((value * 20.0).ceil() as u64)
}

/// Canonical native actor names may omit the Minecraft namespace.
fn actor(name: &str) -> Result<ActorType, RangedError> {
    let full = if name.contains(':') {
        name.to_owned()
    } else {
        format!("minecraft:{name}")
    };
    ActorType::from_name(&full).ok_or(RangedError::Identifier(full))
}

/// Compile every supported shooter field; magic and potion overrides fail closed.
pub(crate) fn shooter(body: body::Shooter) -> Result<ShooterConfig, RangedError> {
    if body.magic {
        return Err(RangedError::Unsupported("magic"));
    }
    if body.aux_val != -1 {
        return Err(RangedError::Unsupported("aux_val"));
    }
    if !body.power.is_finite() || body.power < 0.0 {
        return Err(RangedError::Bounds("power"));
    }
    let default = (!body.def.is_empty())
        .then(|| actor(&body.def))
        .transpose()?;
    let choices = body
        .projectiles
        .into_iter()
        .map(|choice| {
            Ok::<_, RangedError>(ProjectileChoice {
                actor: actor(&choice.def)?,
                filter: choice.filters.as_ref().map(filter).transpose()?,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    if default.is_none() && choices.is_empty() {
        return Err(RangedError::EmptyShooter);
    }
    Ok(ShooterConfig {
        default,
        choices: choices.into(),
        power: (body.power > 0.0).then_some(f64::from(body.power)),
        sound: (!body.sound.is_empty()).then(|| Arc::from(body.sound)),
    })
}

/// Compile range, aim, movement and charge state with explicit unsupported options.
fn ranged(body: body::BehaviorRangedAttack) -> Result<RangedAttackGoal, RangedError> {
    if body.swing {
        return Err(RangedError::Unsupported("swing"));
    }
    if body.charge_charged_trigger != 0.0 {
        return Err(RangedError::Unsupported("charge_charged_trigger"));
    }
    let charge = ticks(body.charge_shoot_trigger, "charge_shoot_trigger")?;
    let min_interval = body.attack_interval_min.unwrap_or(body.attack_interval.min);
    let max_interval = body.attack_interval_max.unwrap_or(body.attack_interval.max);
    let attack_interval = if charge > 0 && min_interval == -1.0 && max_interval == -1.0 {
        (0, 0)
    } else {
        (
            ticks(min_interval, "attack_interval")?,
            ticks(max_interval, "attack_interval")?,
        )
    };
    if attack_interval.0 > attack_interval.1 {
        return Err(RangedError::Bounds("attack_interval"));
    }
    let min = body.attack_radius_min.unwrap_or(body.attack_range.min);
    let max = body.attack_radius.unwrap_or(body.attack_range.max);
    if !min.is_finite() || !max.is_finite() || min < 0.0 || max <= 0.0 || min > max {
        return Err(RangedError::Bounds("attack_range"));
    }
    if !body.ranged_fov.is_finite() || !(0.0..=360.0).contains(&body.ranged_fov) {
        return Err(RangedError::Bounds("ranged_fov"));
    }
    if !body.speed_multiplier.is_finite() || body.speed_multiplier <= 0.0 {
        return Err(RangedError::Bounds("speed_multiplier"));
    }
    if [body.x_max_rotation, body.y_max_head_rotation]
        .iter()
        .any(|v| !v.is_finite() || !(0.0..=180.0).contains(v))
    {
        return Err(RangedError::Bounds("rotation"));
    }
    let burst_interval = ticks(body.burst_interval, "burst_interval")?;
    if !(1..=1024).contains(&body.burst_shots)
        || (charge > 0 && body.burst_shots > 1 && burst_interval == 0)
    {
        return Err(RangedError::Bounds("burst"));
    }
    Ok(RangedAttackGoal {
        priority: body.priority,
        controls: controls(&body.control_flags, GoalControls::MOVE | GoalControls::LOOK),
        attack_interval,
        min_range: f64::from(min),
        max_range: f64::from(max),
        burst_interval,
        burst_shots: body.burst_shots as u16,
        charge_ticks: charge as i64,
        fov: body.ranged_fov,
        speed_multiplier: body.speed_multiplier,
        sight_ticks: ticks(body.target_in_sight_time, "target_in_sight_time")?,
        movement_mode: body.in_range_movement_mode,
        x_max_rotation: body.x_max_rotation,
        y_max_rotation: body.y_max_head_rotation,
        set_persistent: body.set_persistent,
    })
}

/// Compile the common event forms through the lifecycle trigger validator.
fn trigger(event: Option<EventTrigger>) -> Result<Option<ActorTrigger>, TriggerError> {
    match event {
        None => Ok(None),
        Some(EventTrigger::Event(name)) => {
            super::triggers::compile(name, FilterSubject::SelfValue, None).map(Some)
        }
        Some(EventTrigger::Trigger(event)) => {
            super::triggers::compile(event.event, event.target, event.filters).map(Some)
        }
    }
}

/// Compile a sensor without assigning behavior to the names of its events.
fn sensor(body: body::TargetNearbySensor) -> Result<TargetNearbySensor, RangedError> {
    if !body.inside_range.is_finite()
        || !body.outside_range.is_finite()
        || body.inside_range < 0.0
        || body.outside_range < body.inside_range
    {
        return Err(RangedError::Bounds("target range"));
    }
    Ok(TargetNearbySensor {
        inside_range: f64::from(body.inside_range),
        outside_range: f64::from(body.outside_range),
        must_see: body.must_see,
        inside: trigger(body.on_inside_range)?,
        outside: trigger(body.on_outside_range)?,
        vision_lost: trigger(body.on_vision_lost_inside_range)?,
    })
}

/// Update the runtime charge overlay while preserving an authored charged marker.
pub(crate) fn charged_flag(entity: &mut EntityWorldMut<'_>, enabled: bool) {
    use jolyne::valentine::{actor_data_i_ds as ids, actor_flags};
    use unastar_data::actor_components::ActorComponent;
    let authored = entity
        .get::<DefinitionState>()
        .is_some_and(|definition| definition.has_actor_component(ActorComponent::IsCharged));
    let words = entity
        .get_mut::<crate::entity::definition::flags::ActorFlags>()
        .map(|mut flags| {
            flags
                .update(actor_flags::CHARGED, enabled || authored)
                .expect("catalogue charged flag");
            *flags.snapshot().words()
        });
    if let (Some(words), Some(mut metadata)) = (
        words,
        entity.get_mut::<crate::entity::definition::metadata::EntityMetadata>(),
    ) {
        metadata.set_long(ids::RESERVED_0, words[0] as i64);
        metadata.set_long(ids::RESERVED_092, words[1] as i64);
    }
}

/// Register real runtimes with removal hooks for their transient state.
pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
) -> Result<(), DefinitionError> {
    insert(builder, shooter)?;
    builder.register(
        |body: body::BehaviorRangedAttack| Ok(ranged(body)?),
        |goal: &RangedAttackGoal, entity| {
            entity.insert(goal.clone());
        },
        |entity| {
            entity.remove::<RangedAttackGoal>();
            charged_flag(entity, false);
        },
    )?;
    builder.register(
        |body: body::TargetNearbySensor| Ok(sensor(body)?),
        |sensor: &TargetNearbySensor, entity| {
            entity.insert((sensor.clone(), TargetNearbyState::default()));
        },
        |entity| {
            entity.remove::<(TargetNearbySensor, TargetNearbyState)>();
        },
    )
}

/// Validate every possible shooter reference against the completed registry.
/// Conditional choices must be supported even when they do not currently match.
pub(crate) fn check_projectiles(
    archetype: &crate::entity::mob::MobArchetype,
    items: &crate::registry::ItemRegistry,
    get: impl Fn(&str) -> Option<Arc<crate::entity::mob::MobArchetype>>,
) -> Result<(), RangedError> {
    use unastar_data::actor_components::{ActorComponent, decode};
    for components in std::iter::once(&archetype.source.components)
        .chain(archetype.source.component_groups.values())
    {
        let Some(source) = components.get(ActorComponent::Shooter.name()) else {
            continue;
        };
        let config = shooter(decode::<body::Shooter>(source).map_err(RangedError::Decode)?)?;
        for actor in config.projectiles() {
            let projectile = get(actor.name()).ok_or(RangedError::Unspawnable(actor.name()))?;
            if !projectile.projectile {
                return Err(RangedError::NotProjectile(actor.name()));
            }
            crate::entity::projectile::config::check_aimed(&projectile, items)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authored_durations_keep_decimal_tick_boundaries_and_reject_bad_options() {
        assert_eq!(ticks(0.3, "burst").unwrap(), 6);
        for invalid in [f32::NAN, f32::INFINITY, -0.1, 86_401.0] {
            assert!(ticks(invalid, "duration").is_err());
        }
        for input in [
            serde_json::json!({"attack_interval": [3, 1]}),
            serde_json::json!({"attack_range": [3, 1]}),
            serde_json::json!({"swing": true}),
            serde_json::json!({"charge_charged_trigger": 1}),
            serde_json::json!({"burst_shots": 0}),
            serde_json::json!({"ranged_fov": 361}),
        ] {
            let definition =
                unastar_data::actor_components::decode::<body::BehaviorRangedAttack>(&input)
                    .unwrap();
            assert!(ranged(definition).is_err(), "{input}");
        }
    }

    #[test]
    fn unsupported_shooter_semantics_and_reversed_sensor_bounds_fail_closed() {
        for input in [
            serde_json::json!({"def": "minecraft:small_fireball", "magic": true}),
            serde_json::json!({"def": "minecraft:small_fireball", "aux_val": 1}),
            serde_json::json!({"def": "unknown:projectile"}),
            serde_json::json!({}),
        ] {
            assert!(
                shooter(unastar_data::actor_components::decode::<body::Shooter>(&input).unwrap())
                    .is_err()
            );
        }
        assert!(
            sensor(
                unastar_data::actor_components::decode::<body::TargetNearbySensor>(
                    &serde_json::json!({"inside_range": 8, "outside_range": 2})
                )
                .unwrap()
            )
            .is_err()
        );
    }
}
