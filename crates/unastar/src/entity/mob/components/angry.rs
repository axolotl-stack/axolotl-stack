//! `minecraft:angry`: a timed angry state (run by `mob::life::anger`).
//!
//! Applying the component makes the mob angry for `duration` seconds (plus a
//! draw in `[-duration_delta, duration_delta]`; -1 lasts until an event
//! calms it), and with `broadcast_anger` the anger spreads at once to the
//! other mobs of the same definition within `broadcast_range`. When the time
//! is up the mob fires `calm_event`. Removing the component ends the anger
//! without the calm event.

use std::sync::Arc;

use bevy_ecs::prelude::*;
use thiserror::Error;
use unastar_data::actor_components::body::{self, ValueOrRange};
use unastar_rules::filter::Filter;

use super::common::{FilterError, SelfEvent, SelfEventError, filter, self_event};
use crate::entity::definition::{ComponentError, ComponentKindsBuilder, DefinitionError};
use crate::entity::mob::constants::TICKS_PER_SECOND;

/// Why a `minecraft:angry` body cannot be compiled.
#[derive(Debug, Error)]
pub enum AngryError {
    #[error("angry sounds are not implemented")]
    Sound,
    #[error("anger exemption filters are not implemented")]
    Filters,
    #[error("broadcasting anger to other families is not implemented")]
    BroadcastTargets,
    #[error("broadcasting anger on attacking or being attacked is not implemented")]
    BroadcastOnHit,
    /// `broadcast_anger_when_dying` (true unless authored false): spreading
    /// the anger when the mob is killed is not implemented.
    #[error("broadcasting anger when dying is not implemented")]
    BroadcastWhenDying,
    #[error("negative anger duration delta {0}")]
    NegativeDelta(i32),
    #[error(transparent)]
    Filter(#[from] FilterError),
    #[error(transparent)]
    Event(#[from] SelfEventError),
}

impl ComponentError for AngryError {}

/// `minecraft:angry`.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct AngryConfig {
    /// Anger time in ticks; `None` lasts until an event calms the mob.
    pub duration_ticks: Option<u32>,
    /// Ticks drawn uniformly from `[-delta, delta]` and added to the time.
    pub duration_delta_ticks: u32,
    /// `broadcast_anger`: the anger spreads to the other mobs of the same
    /// definition within `broadcast_range` when it starts.
    pub broadcast: bool,
    pub broadcast_range: f64,
    /// Receivers must pass this (the receiver is `self`).
    pub broadcast_filter: Option<Arc<Filter>>,
    /// Fired when the anger time is up.
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
    if b.angry_sound.is_some()
        || b.sound_interval
            .as_ref()
            .is_some_and(|interval| !matches!(interval, ValueOrRange::Value(v) if *v == 0.0))
    {
        return Err(AngryError::Sound);
    }
    if b.filters.is_some() {
        return Err(AngryError::Filters);
    }
    if b.broadcast_targets.as_ref().is_some_and(|t| !t.is_empty()) {
        return Err(AngryError::BroadcastTargets);
    }
    if b.broadcast_anger_on_attack || b.broadcast_anger_on_being_attacked {
        return Err(AngryError::BroadcastOnHit);
    }
    if b.broadcast_anger_when_dying {
        return Err(AngryError::BroadcastWhenDying);
    }
    if b.duration_delta < 0 {
        return Err(AngryError::NegativeDelta(b.duration_delta));
    }
    let seconds_to_ticks = |seconds: i32| (seconds as f32 * TICKS_PER_SECOND) as u32;
    Ok(AngryConfig {
        duration_ticks: (b.duration >= 0).then(|| seconds_to_ticks(b.duration)),
        duration_delta_ticks: seconds_to_ticks(b.duration_delta),
        broadcast: b.broadcast_anger,
        broadcast_range: f64::from(b.broadcast_range),
        broadcast_filter: b
            .broadcast_filters
            .as_ref()
            .map(filter)
            .transpose()?
            .map(Arc::new),
        calm_event: b.calm_event.as_ref().map(self_event).transpose()?,
    })
}
