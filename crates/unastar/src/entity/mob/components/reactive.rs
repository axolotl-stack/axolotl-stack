//! Timed anger and gaze detection compiled from typed actor components.

use std::sync::Arc;

use bevy_ecs::prelude::*;
use jolyne::valentine::{actor_data_i_ds as ids, actor_flags};
use rand::Rng;
use thiserror::Error;
use unastar_data::actor_components::body::{
    self, EventTrigger, FilterSubject, LookLocation, LookLocationName, LookTarget, LookTargetMode,
};
use unastar_rules::filter::Filter;

use super::common::{FilterError, filter, insert};
use super::triggers::{ActorTrigger, TriggerError};
use crate::ecs::TickCounter;
use crate::entity::EntityRandom;
use crate::entity::definition::flags::ActorFlags;
use crate::entity::definition::metadata::EntityMetadata;
use crate::entity::definition::{ComponentError, ComponentKindsBuilder, DefinitionError};

/// Invalid authored values and explicitly unsupported reactive options.
#[derive(Debug, Error)]
pub enum ReactiveError {
    #[error("{0} must be finite and nonnegative, with ordered interval bounds")]
    Bounds(&'static str),
    #[error("anger broadcasting is not implemented")]
    Broadcasting,
    #[error("gaze obstruction mode {0:?} is not implemented; use collision")]
    Obstruction(String),
    #[error("gaze field_of_view must be between zero and 360 degrees")]
    Fov,
    #[error(transparent)]
    Filter(#[from] FilterError),
    #[error(transparent)]
    Trigger(#[from] TriggerError),
}
impl ComponentError for ReactiveError {}

/// Immutable timed anger configuration shared by the runtime state.
#[derive(Debug, Clone)]
pub struct AngerConfig {
    pub duration_ticks: u64,
    pub delta_ticks: u64,
    pub calm_event: Option<ActorTrigger>,
    pub filter: Option<Filter>,
    pub sound: Option<Arc<str>>,
    pub sound_interval: (u64, u64),
}

/// One composed angry state; removal also removes the replicated angry flag.
#[derive(Component, Debug, Clone)]
pub struct Angry {
    pub config: Arc<AngerConfig>,
    pub until_tick: u64,
    pub next_sound_tick: u64,
    pub calmed: bool,
}

/// Whether gaze can assign a target and continue scanning afterward.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GazeTarget {
    Never,
    OnceStop,
    OnceKeep,
}

/// A body-relative line-of-sight point, represented by a height fraction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GazeLocation {
    pub fraction: f64,
    pub offset: f64,
}

/// `minecraft:looked_at`: continuous, filtered gaze detection.
#[derive(Component, Debug, Clone)]
pub struct LookedAt {
    pub half_fov: f64,
    pub filter: Option<Filter>,
    pub players_only: bool,
    pub locations: Arc<[GazeLocation]>,
    pub cooldown: (u64, u64),
    pub looked: Option<ActorTrigger>,
    pub not_looked: Option<ActorTrigger>,
    pub duration: u64,
    pub scale_by_distance: bool,
    pub radius: f64,
    pub target: GazeTarget,
}

/// Convert finite seconds to ticks without admitting an overflowing deadline.
fn ticks(seconds: f32, name: &'static str) -> Result<u64, ReactiveError> {
    let ticks = f64::from(seconds * 20.0);
    if !ticks.is_finite() || ticks < 0.0 || ticks > u64::MAX as f64 / 4.0 {
        return Err(ReactiveError::Bounds(name));
    }
    Ok(ticks.ceil() as u64)
}

/// Compile an optional event in either published event-reference form.
fn event(event: Option<EventTrigger>) -> Result<Option<ActorTrigger>, TriggerError> {
    match event {
        None => Ok(None),
        Some(EventTrigger::Event(event)) => {
            super::triggers::compile(event, FilterSubject::SelfValue, None).map(Some)
        }
        Some(EventTrigger::Trigger(trigger)) => {
            super::triggers::compile(trigger.event, trigger.target, trigger.filters).map(Some)
        }
    }
}

/// Validate and convert a delay range once, before runtime composition.
fn interval(range: body::FloatRange, name: &'static str) -> Result<(u64, u64), ReactiveError> {
    if range.min > range.max {
        return Err(ReactiveError::Bounds(name));
    }
    Ok((ticks(range.min, name)?, ticks(range.max, name)?))
}

/// Compile the timed state, rejecting unimplemented broadcast requests.
fn anger(body: body::Angry) -> Result<Arc<AngerConfig>, ReactiveError> {
    if body.broadcast_anger
        || body.broadcast_anger_on_attack
        || body.broadcast_anger_on_being_attacked
        || body.broadcast_filters.is_some()
        || !body.broadcast_targets.is_empty()
        || !body.broadcast_anger_when_dying
        || body.broadcast_range != 20.0
    {
        return Err(ReactiveError::Broadcasting);
    }
    Ok(Arc::new(AngerConfig {
        duration_ticks: ticks(body.duration, "duration")?,
        delta_ticks: ticks(body.duration_delta, "duration_delta")?,
        calm_event: event(body.calm_event)?,
        filter: body.filters.as_ref().map(filter).transpose()?,
        sound: body
            .angry_sound
            .filter(|sound| !sound.is_empty())
            .map(Arc::from),
        sound_interval: interval(body.sound_interval, "sound_interval")?,
    }))
}

/// Compile points, detection geometry, timing and the target behavior.
fn gaze(body: body::LookedAt) -> Result<LookedAt, ReactiveError> {
    if body.line_of_sight_obstruction_type != "collision" {
        return Err(ReactiveError::Obstruction(
            body.line_of_sight_obstruction_type,
        ));
    }
    if !body.field_of_view.is_finite() || !(0.0..=360.0).contains(&body.field_of_view) {
        return Err(ReactiveError::Fov);
    }
    if !body.search_radius.is_finite() || body.search_radius < 0.0 {
        return Err(ReactiveError::Bounds("search_radius"));
    }
    let mut locations = Vec::new();
    for location in body.look_at_locations {
        let (name, offset) = match location {
            LookLocation::Name(name) => (name, 0.0),
            LookLocation::Object(location) => (location.location, location.vertical_offset),
        };
        if !offset.is_finite() {
            return Err(ReactiveError::Bounds("vertical_offset"));
        }
        locations.push(GazeLocation {
            fraction: match name {
                LookLocationName::Head => super::super::constants::MOB_EYE_HEIGHT_RATIO,
                LookLocationName::Body => 0.5,
                LookLocationName::Feet => 0.0,
            },
            offset: f64::from(offset),
        });
    }
    if locations.is_empty() {
        locations.push(GazeLocation {
            fraction: super::super::constants::MOB_EYE_HEIGHT_RATIO,
            offset: 0.0,
        });
    }
    let target = match body.set_target {
        LookTarget::Mode(LookTargetMode::Never) | LookTarget::Legacy(false) => GazeTarget::Never,
        LookTarget::Mode(LookTargetMode::OnceAndStopScanning) | LookTarget::Legacy(true) => {
            GazeTarget::OnceStop
        }
        LookTarget::Mode(LookTargetMode::OnceAndKeepScanning) => GazeTarget::OnceKeep,
    };
    Ok(LookedAt {
        half_fov: f64::from(body.field_of_view).to_radians() * 0.5,
        filter: body.filters.as_ref().map(filter).transpose()?,
        players_only: body.find_players_only,
        locations: locations.into(),
        cooldown: interval(body.looked_at_cooldown, "looked_at_cooldown")?,
        looked: event(body.looked_at_event)?,
        not_looked: event(body.not_looked_at_event)?,
        duration: ticks(body.min_looked_at_duration, "min_looked_at_duration")?,
        scale_by_distance: body.scale_fov_by_distance,
        radius: f64::from(body.search_radius),
        target,
    })
}

/// Update both the authoritative flag and its coalesced wire words.
pub(crate) fn angry_flag(entity: &mut EntityWorldMut<'_>, enabled: bool) {
    let words = entity.get_mut::<ActorFlags>().map(|mut flags| {
        flags
            .update(actor_flags::ANGRY, enabled)
            .expect("catalogue flag");
        *flags.snapshot().words()
    });
    if let (Some(words), Some(mut metadata)) = (words, entity.get_mut::<EntityMetadata>()) {
        metadata.set_long(ids::RESERVED_0, words[0] as i64);
        metadata.set_long(ids::RESERVED_092, words[1] as i64);
    }
}

/// Register real reactive behavior factories; unsupported options fail startup.
pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
) -> Result<(), DefinitionError> {
    builder.register(
        |body: body::Angry| Ok(anger(body)?),
        |config: &Arc<AngerConfig>, entity| {
            let (now, delta) = entity.world_scope(|world| {
                let now = world.resource::<TickCounter>().current;
                let delta = if config.delta_ticks > 0 {
                    world
                        .get_resource_or_init::<EntityRandom>()
                        .0
                        .gen_range(0..=config.delta_ticks.saturating_mul(2))
                } else {
                    0
                };
                (now, delta)
            });
            let span = config
                .duration_ticks
                .saturating_add(delta)
                .saturating_sub(config.delta_ticks);
            entity.insert(Angry {
                config: Arc::clone(config),
                until_tick: now.saturating_add(span),
                next_sound_tick: now,
                calmed: false,
            });
            angry_flag(entity, true);
        },
        |entity| {
            entity.remove::<Angry>();
            angry_flag(entity, false);
        },
    )?;
    insert(builder, gaze)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use unastar_data::actor_components::decode;

    #[test]
    fn closed_bodies_apply_published_defaults_and_reject_unknown_fields() {
        let defaults = decode::<body::Angry>(&json!({})).unwrap();
        assert_eq!(defaults.duration, 25.0);
        assert_eq!(anger(defaults).unwrap().duration_ticks, 500);
        let defaults = gaze(decode::<body::LookedAt>(&json!({})).unwrap()).unwrap();
        assert_eq!(defaults.radius, 10.0);
        assert_eq!(defaults.target, GazeTarget::Never);
        assert!(decode::<body::LookedAt>(&json!({"search_raduis":10})).is_err());
        assert!(decode::<body::Angry>(&json!({"duraton":25})).is_err());
    }

    #[test]
    fn unsupported_behavior_and_invalid_bounds_fail_instead_of_becoming_inert() {
        for value in [
            json!({"broadcast_anger":true}),
            json!({"duration":-1}),
            json!({"duration_delta":-1}),
            json!({"sound_interval":[2,1]}),
            json!({"duration":1e30}),
        ] {
            assert!(
                anger(decode::<body::Angry>(&value).unwrap()).is_err(),
                "{value}"
            );
        }
        for value in [
            json!({"line_of_sight_obstruction_type":"outline"}),
            json!({"field_of_view":361}),
            json!({"search_radius":-1}),
            json!({"looked_at_cooldown":[2,1]}),
        ] {
            assert!(
                gaze(decode::<body::LookedAt>(&value).unwrap()).is_err(),
                "{value}"
            );
        }
    }
}
