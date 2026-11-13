//! `minecraft:angry`: a timed angry state (run by `mob::life::anger`).
//!
//! Applying the component starts the anger. While it lasts the mob shows
//! the `ANGRY` actor flag, plays `angry_sound` every `sound_interval`, and
//! spreads its target to other mobs (`broadcast_anger` once, and on every
//! attack it makes or receives with `broadcast_anger_on_attack` /
//! `broadcast_anger_on_being_attacked`). When the time is up, or the mob can
//! no longer be angry at its target, it drops the target and fires
//! `calm_event`. Removing the component ends the anger without the calm
//! event.

use std::sync::Arc;

use bevy_ecs::prelude::*;
use thiserror::Error;
use unastar_data::actor_components::body::{self, ValueOrRange};
use unastar_rules::filter::Filter;

use super::common::{FilterError, SelfEvent, SelfEventError, filter, self_event};
use crate::entity::definition::{ComponentError, ComponentKindsBuilder, DefinitionError};

/// Why a `minecraft:angry` body cannot be compiled.
#[derive(Debug, Error)]
pub enum AngryError {
    #[error("angry sound interval {min}..{max} is not a range of seconds")]
    SoundInterval { min: f32, max: f32 },
    #[error(transparent)]
    Filter(#[from] FilterError),
    #[error(transparent)]
    Event(#[from] SelfEventError),
}

impl ComponentError for AngryError {}

/// `minecraft:angry`.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct AngryConfig {
    /// `duration` and `duration_delta` in seconds. Each start or restart
    /// draws `duration - delta + random(2 × delta + 1)` seconds; a negative
    /// draw lasts until an event calms the mob (`duration` -1).
    pub duration: i32,
    pub duration_delta: i32,
    /// `broadcast_anger`: the anger spreads once, when it starts.
    pub broadcast: bool,
    /// `broadcast_anger_on_attack` / `_on_being_attacked`: it spreads again
    /// on every hit the mob deals / takes from another actor.
    pub broadcast_on_attack: bool,
    pub broadcast_on_being_attacked: bool,
    /// `broadcast_anger_when_dying`: a dying mob still spreads its anger.
    pub broadcast_when_dying: bool,
    pub broadcast_range: f64,
    /// `broadcast_targets`: the families the anger spreads to; empty: the
    /// mob's own native actor type.
    pub broadcast_targets: Arc<[Arc<str>]>,
    /// `broadcast_filters`: a receiver must pass it (the receiver is
    /// `self`, the angry mob `other`).
    pub broadcast_filter: Option<Arc<Filter>>,
    /// `filters`: the target must pass it for the mob to stay angry (the
    /// angry mob is `self`, the target `other`); with it, a mob without a
    /// target cannot stay angry either.
    pub filter: Option<Arc<Filter>>,
    /// `angry_sound`, played while angry every `sound_interval` seconds.
    pub sound: Option<Arc<str>>,
    pub sound_interval: (f32, f32),
    /// Fired when the anger ends.
    pub calm_event: Option<SelfEvent>,
}

/// Register the `minecraft:angry` kind.
pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
) -> Result<(), DefinitionError> {
    builder.register(
        compile,
        |config: &AngryConfig, entity| crate::entity::mob::life::anger::start(entity, config),
        crate::entity::mob::life::anger::stop,
    )
}

fn compile(
    b: body::Angry,
) -> Result<AngryConfig, crate::entity::definition::ComponentCompileError> {
    compile_angry(b).map_err(Into::into)
}

fn compile_angry(b: body::Angry) -> Result<AngryConfig, AngryError> {
    let sound_interval = match b.sound_interval {
        None => (0.0, 0.0),
        Some(ValueOrRange::Value(value)) => (value, value),
        Some(ValueOrRange::Range(range)) => (range.range_min, range.range_max),
    };
    let (min, max) = sound_interval;
    if !(min.is_finite() && max.is_finite() && min >= 0.0 && max >= min) {
        return Err(AngryError::SoundInterval { min, max });
    }
    let optional_filter = |json: &Option<unastar_data::actor_components::FilterJson>| {
        json.as_ref()
            .map(filter)
            .transpose()
            .map(|filter| filter.map(Arc::new))
    };
    Ok(AngryConfig {
        duration: b.duration,
        duration_delta: b.duration_delta,
        broadcast: b.broadcast_anger,
        broadcast_on_attack: b.broadcast_anger_on_attack,
        broadcast_on_being_attacked: b.broadcast_anger_on_being_attacked,
        broadcast_when_dying: b.broadcast_anger_when_dying,
        broadcast_range: f64::from(b.broadcast_range),
        broadcast_targets: b
            .broadcast_targets
            .unwrap_or_default()
            .into_iter()
            .map(Arc::from)
            .collect(),
        broadcast_filter: optional_filter(&b.broadcast_filters)?,
        filter: optional_filter(&b.filters)?,
        sound: b
            .angry_sound
            .filter(|sound| !sound.is_empty())
            .map(Arc::from),
        sound_interval,
        calm_event: b.calm_event.as_ref().map(self_event).transpose()?,
    })
}
