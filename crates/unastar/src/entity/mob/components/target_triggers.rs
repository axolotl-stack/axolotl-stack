//! Target triggers: `minecraft:on_target_acquired` and
//! `minecraft:on_target_escape`.
//!
//! The target goals (`mob::ai::targeting`) decide the mob's target; when the
//! target they mirror to `AttackTarget` changes to a new actor the mob fires
//! its `on_target_acquired` event, and when it is dropped its
//! `on_target_escape` event, after the AI tick (see `mob::ai`). The trigger's
//! filter sees the target as `other`. A trigger without an event (vanilla's
//! `{}` in angry groups) is present but fires nothing, which is how a group
//! switches the trigger off.

use bevy_ecs::prelude::*;
use thiserror::Error;
use unastar_data::actor_components::body::{self, FilterSubject};
use unastar_rules::filter::Filter;

use super::common::{FilterError, SelfEvent, filter, insert};
use crate::entity::definition::{ComponentError, ComponentKindsBuilder, DefinitionError};

/// Why a target trigger cannot be compiled.
#[derive(Debug, Error)]
pub enum TargetTriggerError {
    #[error("trigger event {event} targets {target:?}; only self is supported")]
    OtherTarget {
        event: String,
        target: FilterSubject,
    },
    #[error(transparent)]
    Filter(#[from] FilterError),
}

impl ComponentError for TargetTriggerError {}

/// An event the mob fires on itself when its target changes.
#[derive(Debug, Clone, PartialEq)]
pub struct TargetTrigger {
    /// `None`: the trigger fires nothing.
    pub event: Option<SelfEvent>,
    /// Evaluated with the target as `other`.
    pub filter: Option<Filter>,
}

/// `minecraft:on_target_acquired`.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct OnTargetAcquired(pub TargetTrigger);

/// `minecraft:on_target_escape`.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct OnTargetEscape(pub TargetTrigger);

/// `minecraft:on_friendly_anger`: fired on a mob another mob's anger spread
/// to (`mob::life::anger`), with the angry mob as `other`.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct OnFriendlyAnger(pub TargetTrigger);

fn trigger(
    event: String,
    filters: Option<&unastar_data::actor_components::FilterJson>,
    target: FilterSubject,
) -> Result<TargetTrigger, TargetTriggerError> {
    if event.is_empty() {
        return Ok(TargetTrigger {
            event: None,
            filter: None,
        });
    }
    if target != FilterSubject::SelfValue {
        return Err(TargetTriggerError::OtherTarget { event, target });
    }
    Ok(TargetTrigger {
        event: Some(SelfEvent(event.into())),
        filter: filters.map(filter).transpose()?,
    })
}

/// Register the target trigger kinds.
pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
) -> Result<(), DefinitionError> {
    insert(builder, |b: body::OnTargetAcquired| {
        trigger(b.event, b.filters.as_ref(), b.target).map(OnTargetAcquired)
    })?;
    insert(builder, |b: body::OnTargetEscape| {
        trigger(b.event, b.filters.as_ref(), b.target).map(OnTargetEscape)
    })?;
    insert(builder, |b: body::OnFriendlyAnger| {
        trigger(b.event, b.filters.as_ref(), b.target).map(OnFriendlyAnger)
    })
}
