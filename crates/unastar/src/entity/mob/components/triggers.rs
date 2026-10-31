//! Authored triggers dispatched after a hit or target change commits.

use std::sync::Arc;

use bevy_ecs::prelude::*;
use thiserror::Error;
use unastar_data::actor_components::body::{self, FilterSubject};
use unastar_rules::filter::Filter;

use super::common::{FilterError, filter, insert};
use crate::entity::definition::{ComponentError, ComponentKindsBuilder, DefinitionError};

/// The receiver of an authored lifecycle event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerTarget {
    SelfActor,
    OtherActor,
}

/// An immutable event and optional condition compiled at startup.
#[derive(Debug, Clone, PartialEq)]
pub struct ActorTrigger {
    pub event: Arc<str>,
    pub target: TriggerTarget,
    pub filter: Option<Filter>,
}

/// Event fired after this actor accepts a hit.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct OnHurt(pub ActorTrigger);

/// Event fired after this actor accepts a player-sourced hit.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct OnHurtByPlayer(pub ActorTrigger);

/// Event fired when a different nonempty attack target is committed.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct OnTargetAcquired(pub ActorTrigger);

/// Why a lifecycle trigger cannot be evaluated by the runtime.
#[derive(Debug, Error)]
pub enum TriggerError {
    #[error("lifecycle event name must not be empty")]
    EmptyEvent,
    #[error("lifecycle trigger target {0:?} is not self or other")]
    Target(FilterSubject),
    #[error(transparent)]
    Filter(#[from] FilterError),
}

impl ComponentError for TriggerError {}

/// Compile one lifecycle trigger without retaining authored JSON.
fn compile(
    event: String,
    target: FilterSubject,
    filters: Option<unastar_data::actor_components::FilterJson>,
) -> Result<ActorTrigger, TriggerError> {
    if event.is_empty() {
        return Err(TriggerError::EmptyEvent);
    }
    let target = match target {
        FilterSubject::SelfValue => TriggerTarget::SelfActor,
        FilterSubject::Other => TriggerTarget::OtherActor,
        other => return Err(TriggerError::Target(other)),
    };
    Ok(ActorTrigger {
        event: Arc::from(event),
        target,
        filter: filters.as_ref().map(filter).transpose()?,
    })
}

/// Register the supported lifecycle components before freezing factories.
pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
) -> Result<(), DefinitionError> {
    insert(builder, |body: body::OnHurt| {
        compile(body.event, body.target, body.filters).map(OnHurt)
    })?;
    insert(builder, |body: body::OnHurtByPlayer| {
        compile(body.event, body.target, body.filters).map(OnHurtByPlayer)
    })?;
    insert(builder, |body: body::OnTargetAcquired| {
        compile(body.event, body.target, body.filters).map(OnTargetAcquired)
    })
}
