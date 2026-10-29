//! `minecraft:looked_at`: the owner notices actors looking at it (run by
//! `mob::life::looked_at`).

use std::sync::Arc;

use bevy_ecs::prelude::*;
use thiserror::Error;
use unastar_data::actor_components::body::{
    self, FilterSubject, LineOfSightObstruction, LookedAtSetTarget, LookedAtTrigger,
};
use unastar_rules::filter::Filter;

use super::common::{FilterError, SelfEvent, filter};
use crate::entity::definition::{ComponentError, ComponentKindsBuilder, DefinitionError};
use crate::entity::mob::constants::{
    LOOKED_AT_MAX_FIELD_OF_VIEW, LOOKED_AT_MIN_FIELD_OF_VIEW, TICKS_PER_SECOND,
};

/// Why a `minecraft:looked_at` body cannot be compiled.
#[derive(Debug, Error)]
pub enum LookedAtError {
    #[error("look_at_locations location {0:?} is not head, body or feet")]
    UnknownLocation(String),
    #[error("looked_at event {event} targets {target:?}; only self is supported")]
    OtherTarget {
        event: String,
        target: FilterSubject,
    },
    #[error("looked_at event {0} has filters, which are not supported")]
    FilteredEvent(String),
    #[error(transparent)]
    Filter(#[from] FilterError),
}

impl ComponentError for LookedAtError {}

/// A point on the owner a looker aims at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachPoint {
    /// The owner's eye height.
    Head,
    /// The middle of the owner's box.
    Body,
    /// The owner's feet.
    Feet,
}

/// One `look_at_locations` entry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LookAtPoint {
    pub point: AttachPoint,
    pub vertical_offset: f64,
}

/// `minecraft:looked_at`, compiled.
#[derive(Component, Debug, Clone)]
pub struct LookedAtConfig {
    pub search_radius: f64,
    pub find_players_only: bool,
    pub set_target: LookedAtSetTarget,
    /// Consecutive ticks a looker must be found before it counts.
    pub min_looked_ticks: u32,
    /// Seconds (min, max) the owner waits after a detection.
    pub cooldown: (f32, f32),
    /// `1 - cos(field_of_view / 2)`: how far the looker's view may stray
    /// from the owner (at one block when `scale_fov_by_distance`).
    pub view_slack: f64,
    pub scale_fov_by_distance: bool,
    /// Which lookers count (`other` is the looker).
    pub filter: Filter,
    pub points: Arc<[LookAtPoint]>,
    /// The block shapes that block the view. The line-of-sight test checks
    /// collision shapes for every type.
    pub obstruction: LineOfSightObstruction,
    pub looked_at_event: Option<SelfEvent>,
    pub not_looked_at_event: Option<SelfEvent>,
}

/// The owner's scan state: the cooldown deadline and the first tick of the
/// current run of consecutive detections.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq)]
#[component(storage = "SparseSet")]
pub struct LookedAtState {
    pub cooldown_until: u64,
    pub looked_since: Option<u64>,
}

/// Register the `minecraft:looked_at` kind.
pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
) -> Result<(), DefinitionError> {
    builder.register(
        |b: body::LookedAt| compile(b).map_err(Into::into),
        |config: &LookedAtConfig, entity| {
            entity.insert((config.clone(), LookedAtState::default()));
        },
        |entity| {
            entity.remove::<(LookedAtConfig, LookedAtState)>();
        },
    )
}

fn compile(b: body::LookedAt) -> Result<LookedAtConfig, LookedAtError> {
    let fov = b
        .field_of_view
        .clamp(LOOKED_AT_MIN_FIELD_OF_VIEW, LOOKED_AT_MAX_FIELD_OF_VIEW);
    let points = b
        .look_at_locations
        .iter()
        .map(|entry| {
            let point = match entry.location.as_str() {
                "head" => AttachPoint::Head,
                "body" => AttachPoint::Body,
                "feet" => AttachPoint::Feet,
                other => return Err(LookedAtError::UnknownLocation(other.to_owned())),
            };
            Ok(LookAtPoint {
                point,
                vertical_offset: f64::from(entry.vertical_offset),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(LookedAtConfig {
        search_radius: f64::from(b.search_radius),
        find_players_only: b.find_players_only,
        set_target: b.set_target,
        min_looked_ticks: (b.min_looked_at_duration * TICKS_PER_SECOND) as u32,
        cooldown: (b.looked_at_cooldown.min, b.looked_at_cooldown.max),
        view_slack: 1.0 - (f64::from(fov).to_radians() / 2.0).cos(),
        scale_fov_by_distance: b.scale_fov_by_distance,
        filter: match &b.filters {
            Some(json) => filter(json)?,
            None => Filter::always(),
        },
        points: points.into(),
        obstruction: b.line_of_sight_obstruction_type,
        looked_at_event: event(b.looked_at_event.as_ref())?,
        not_looked_at_event: event(b.not_looked_at_event.as_ref())?,
    })
}

/// A trigger with an empty event fires nothing.
fn event(trigger: Option<&LookedAtTrigger>) -> Result<Option<SelfEvent>, LookedAtError> {
    let Some(trigger) = trigger.filter(|t| !t.event.is_empty()) else {
        return Ok(None);
    };
    let target = trigger.target.unwrap_or(FilterSubject::SelfValue);
    if target != FilterSubject::SelfValue {
        return Err(LookedAtError::OtherTarget {
            event: trigger.event.clone(),
            target,
        });
    }
    if trigger.filters.is_some() {
        return Err(LookedAtError::FilteredEvent(trigger.event.clone()));
    }
    Ok(Some(SelfEvent(Arc::from(trigger.event.as_str()))))
}
