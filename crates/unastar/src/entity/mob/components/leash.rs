//! `minecraft:leashable`: how a lead holds the mob (run by
//! `mob::interaction::leash`).

use bevy_ecs::prelude::*;
use thiserror::Error;
use unastar_data::actor_components::body;

use super::common::{SelfEvent, SelfEventError, insert, self_event};
use crate::entity::definition::{ComponentError, ComponentKindsBuilder, DefinitionError};

/// Break distance when the definition leaves `max_distance` at its
/// documented default of 0 (public reference, unmeasured: 10 blocks).
pub const DEFAULT_MAX_DISTANCE: f64 = 10.0;

/// Why a `minecraft:leashable` body cannot be compiled.
#[derive(Debug, Error)]
pub enum LeashError {
    #[error("leash presets (per-holder distances and spring types) are not implemented")]
    Presets,
    #[error(transparent)]
    Event(#[from] SelfEventError),
}

impl ComponentError for LeashError {}

/// `minecraft:leashable`.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct LeashableConfig {
    /// Beyond this distance from the holder the mob walks towards it.
    pub soft_distance: f64,
    /// Beyond this distance the lead pulls the mob.
    pub hard_distance: f64,
    /// Beyond this distance the lead breaks.
    pub max_distance: f64,
    /// Another player may take over the lead.
    pub can_be_stolen: bool,
    /// Fired on the mob when it is leashed.
    pub on_leash: Option<SelfEvent>,
    /// Fired on the mob when its lead comes off.
    pub on_unleash: Option<SelfEvent>,
}

/// Register the leash component kind.
pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
) -> Result<(), DefinitionError> {
    insert(builder, |b: body::Leashable| {
        if b.presets
            .as_ref()
            .is_some_and(|presets| !presets.is_empty())
        {
            return Err(LeashError::Presets);
        }
        Ok(LeashableConfig {
            soft_distance: f64::from(b.soft_distance),
            hard_distance: f64::from(b.hard_distance),
            max_distance: if b.max_distance > 0.0 {
                f64::from(b.max_distance)
            } else {
                DEFAULT_MAX_DISTANCE
            },
            can_be_stolen: b.can_be_stolen,
            on_leash: b.on_leash.as_ref().map(self_event).transpose()?,
            on_unleash: b.on_unleash.as_ref().map(self_event).transpose()?,
        })
    })
}
