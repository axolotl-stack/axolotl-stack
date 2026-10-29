//! `minecraft:environment_sensor`, `minecraft:timer` and
//! `minecraft:target_nearby_sensor`: events fired from the mob's
//! surroundings, from a countdown and from its target's distance (run by
//! `mob::life::sensors`).

use std::sync::Arc;

use bevy_ecs::prelude::*;
use rand::Rng;
use thiserror::Error;
use unastar_data::actor_components::body::{
    self, ActorDefinitionTrigger, FilterSubject, TimerTime,
};
use unastar_rules::filter::Filter;

use super::common::{FilterError, SelfEvent, filter};
use crate::ecs::TickCounter;
use crate::entity::EntityRandom;
use crate::entity::definition::{
    ComponentCompileError, ComponentError, ComponentKindsBuilder, DefinitionError,
};
use crate::entity::mob::constants::TICKS_PER_SECOND;

/// Why a sensor or timer body cannot be compiled.
#[derive(Debug, Error)]
pub enum SensorError {
    #[error("event {event} targets {target:?}; only self is supported")]
    OtherTarget {
        event: String,
        target: FilterSubject,
    },
    #[error("timer time_down_event filters are not supported")]
    FilteredTimer,
    #[error("timer random_time_choices weights are all zero")]
    NoTimerChoice,
    #[error(transparent)]
    Filter(#[from] FilterError),
    #[error(transparent)]
    Event(#[from] super::common::SelfEventError),
}

impl ComponentError for SensorError {}

/// One `environment_sensor` trigger: fire `event` on the mob every tick its
/// filter passes.
#[derive(Debug, Clone, PartialEq)]
pub struct EnvironmentTrigger {
    pub filter: Filter,
    pub event: SelfEvent,
}

/// `minecraft:environment_sensor`.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct EnvironmentSensorConfig {
    pub triggers: Arc<[EnvironmentTrigger]>,
}

/// How long a timer runs, in ticks.
#[derive(Debug, Clone, PartialEq)]
pub enum TimerSpan {
    /// Uniform in `[min, max)` (exactly `min` when they are equal).
    Range { min: i32, max: i32 },
    /// `random_time_choices`: weighted spans.
    Choices(Arc<[(i32, i32)]>),
}

/// `minecraft:timer`.
#[derive(Debug, Clone, PartialEq)]
pub struct TimerConfig {
    pub looping: bool,
    pub random_interval: bool,
    pub span: TimerSpan,
    pub event: SelfEvent,
}

/// A running `minecraft:timer`: the definition plus its countdown, started
/// when the component is added.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct Timer {
    pub config: Arc<TimerConfig>,
    /// The span drawn for the current run, in ticks.
    pub span_ticks: i32,
    /// The server tick the timer fires at.
    pub deadline: u64,
    /// The timer has fired at least once.
    pub fired: bool,
}

/// `minecraft:target_nearby_sensor`: events fired as the mob's current
/// target crosses `inside_range` / `outside_range` (box-to-box gaps).
#[derive(Component, Debug, Clone, PartialEq)]
pub struct TargetNearbySensorConfig {
    pub inside_range: f64,
    pub outside_range: f64,
    /// The target only counts as inside while the mob sees it.
    pub must_see: bool,
    pub on_inside_range: Option<SelfEvent>,
    pub on_outside_range: Option<SelfEvent>,
    pub on_vision_lost_inside_range: Option<SelfEvent>,
}

/// What a `minecraft:target_nearby_sensor` saw last tick; reset whenever
/// the mob has no target. Absent means all false.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[component(storage = "SparseSet")]
pub struct TargetNearbyState {
    pub seen: bool,
    pub inside: bool,
    pub outside: bool,
}

impl TimerSpan {
    /// A span drawn as the native timer does.
    pub fn draw(&self, rng: &mut impl Rng) -> i32 {
        match self {
            Self::Range { min, max } => *min + rng.gen_range(0..(*max - *min).max(1)),
            Self::Choices(choices) => {
                let total: i32 = choices.iter().map(|(_, weight)| *weight).sum();
                let mut roll = rng.gen_range(0..total);
                for (ticks, weight) in choices.iter() {
                    if roll < *weight {
                        return *ticks;
                    }
                    roll -= weight;
                }
                unreachable!("roll below the total weight")
            }
        }
    }
}

/// Register the sensor and timer component kinds.
pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
) -> Result<(), DefinitionError> {
    super::common::insert(builder, |b: body::EnvironmentSensor| {
        let triggers = b
            .triggers
            .into_iter()
            .map(|trigger| {
                Ok(EnvironmentTrigger {
                    filter: match &trigger.filters {
                        Some(json) => filter(json)?,
                        None => Filter::always(),
                    },
                    event: event(&trigger)?,
                })
            })
            .collect::<Result<Vec<_>, SensorError>>()?;
        Ok::<_, SensorError>(EnvironmentSensorConfig {
            triggers: triggers.into(),
        })
    })?;
    builder.register(
        |b: body::Timer| compile_timer(b).map_err(ComponentCompileError::from),
        start_timer,
        |entity| {
            entity.remove::<Timer>();
        },
    )?;
    // A replaced sensor starts from a fresh state.
    builder.register(
        |b: body::TargetNearbySensor| {
            compile_target_nearby_sensor(b).map_err(ComponentCompileError::from)
        },
        |value: &TargetNearbySensorConfig, entity| {
            entity.insert(value.clone());
        },
        |entity| {
            entity.remove::<(TargetNearbySensorConfig, TargetNearbyState)>();
        },
    )
}

fn compile_target_nearby_sensor(
    b: body::TargetNearbySensor,
) -> Result<TargetNearbySensorConfig, SensorError> {
    let event = |trigger: &Option<body::EventTrigger>| {
        trigger
            .as_ref()
            .map(super::common::self_event)
            .transpose()
    };
    Ok(TargetNearbySensorConfig {
        inside_range: f64::from(b.inside_range),
        outside_range: f64::from(b.outside_range),
        must_see: b.must_see,
        on_inside_range: event(&b.on_inside_range)?,
        on_outside_range: event(&b.on_outside_range)?,
        on_vision_lost_inside_range: event(&b.on_vision_lost_inside_range)?,
    })
}

fn event(trigger: &ActorDefinitionTrigger) -> Result<SelfEvent, SensorError> {
    if trigger.target != FilterSubject::SelfValue {
        return Err(SensorError::OtherTarget {
            event: trigger.event.clone(),
            target: trigger.target,
        });
    }
    Ok(SelfEvent(Arc::from(trigger.event.as_str())))
}

fn compile_timer(b: body::Timer) -> Result<Arc<TimerConfig>, SensorError> {
    if b.time_down_event.filters.is_some() {
        return Err(SensorError::FilteredTimer);
    }
    let ticks = |seconds: f32| (seconds * TICKS_PER_SECOND) as i32;
    let span = if b.random_time_choices.is_empty() {
        let (min, max) = match b.time {
            TimerTime::Seconds(seconds) => (seconds, seconds),
            TimerTime::Range([min, max]) => (min, max),
        };
        TimerSpan::Range {
            min: ticks(min),
            max: ticks(max),
        }
    } else {
        if b.random_time_choices
            .iter()
            .all(|choice| choice.weight <= 0)
        {
            return Err(SensorError::NoTimerChoice);
        }
        TimerSpan::Choices(
            b.random_time_choices
                .iter()
                .filter(|choice| choice.weight > 0)
                .map(|choice| (ticks(choice.value), choice.weight))
                .collect::<Vec<_>>()
                .into(),
        )
    };
    Ok(Arc::new(TimerConfig {
        looping: b.looping,
        random_interval: b.random_interval,
        span,
        event: event(&b.time_down_event)?,
    }))
}

/// The timer starts counting when the component is added.
fn start_timer(config: &Arc<TimerConfig>, entity: &mut EntityWorldMut<'_>) {
    let (now, span_ticks) = entity.world_scope(|world| {
        let now = world.resource::<TickCounter>().current;
        let span = config
            .span
            .draw(&mut world.get_resource_or_init::<EntityRandom>().0);
        (now, span)
    });
    entity.insert(Timer {
        config: Arc::clone(config),
        span_ticks,
        deadline: now + span_ticks.max(0) as u64,
        fired: false,
    });
}
