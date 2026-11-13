//! `minecraft:looked_at` (run by `mob::ai::looked_at`): the enderman's
//! stare.

use bevy_ecs::prelude::*;
use thiserror::Error;
use unastar_data::actor_components::body::{self, LineOfSightObstruction, LookedAtSetTarget};
use unastar_rules::filter::Filter;

use super::common::{FilterError, filter, insert};
use crate::entity::definition::{ComponentError, ComponentKindsBuilder, DefinitionError};

/// `minecraft:looked_at`: players looking at the mob for long enough
/// become its target.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct LookedAtConfig {
    /// Blocks between the looking player's eyes and the mob's eyes.
    pub search_radius: f64,
    /// The cone's `1 - cos` at one block, `1 - cos(field_of_view / 2)`
    /// (`mob::life::looked_at::in_view`).
    pub cone_slack: f64,
    /// Which players count (`other` is the player).
    pub filter: Option<Filter>,
    /// Ticks a player must look without a break.
    pub min_ticks: u32,
    pub set_target: LookedAtSetTarget,
}

/// A `minecraft:looked_at` body the runtime cannot honour.
#[derive(Debug, Error)]
pub enum LookedAtError {
    #[error(transparent)]
    Filter(#[from] FilterError),
    #[error("looked_at: {0} is not implemented")]
    Unsupported(&'static str),
}

impl ComponentError for LookedAtError {}

/// Register the looked-at component kind.
pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
) -> Result<(), DefinitionError> {
    insert(builder, |b: body::LookedAt| {
        let unsupported = |what| Err(LookedAtError::Unsupported(what));
        if !b.find_players_only {
            return unsupported("looking for mobs (find_players_only false)");
        }
        if b.looked_at_event.is_some() || b.not_looked_at_event.is_some() {
            return unsupported("looked_at_event and not_looked_at_event");
        }
        if b.look_at_locations.is_some() {
            return unsupported("look_at_locations");
        }
        if b.looked_at_cooldown.is_some() {
            return unsupported("looked_at_cooldown");
        }
        if b.line_of_sight_obstruction_type != LineOfSightObstruction::Collision {
            return unsupported("a line_of_sight_obstruction_type other than collision");
        }
        // Only the narrowing cone is measured (the enderman's).
        if !b.scale_fov_by_distance {
            return unsupported("scale_fov_by_distance false");
        }
        Ok(LookedAtConfig {
            search_radius: f64::from(b.search_radius),
            cone_slack: 1.0 - (f64::from(b.field_of_view) / 2.0).to_radians().cos(),
            filter: b.filters.as_ref().map(filter).transpose()?,
            min_ticks: (b.min_looked_at_duration.max(0.0) * 20.0).round() as u32,
            set_target: b.set_target,
        })
    })
}
