//! `minecraft:despawn`: the natural despawn rules (run by
//! `mob::life::despawn`).

use bevy_ecs::prelude::*;
use thiserror::Error;
use unastar_data::actor_components::body;

use crate::entity::definition::{ComponentError, ComponentKindsBuilder, DefinitionError};
use crate::entity::mob::life::despawn::Inactivity;

/// Why a `minecraft:despawn` body cannot be compiled.
#[derive(Debug, Error)]
pub enum DespawnError {
    #[error("despawn filters are not implemented")]
    Filters,
}

impl ComponentError for DespawnError {}

/// `minecraft:despawn`.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct DespawnConfig {
    /// `(min_distance, max_distance)` blocks, when `despawn_from_distance` is present.
    pub from_distance: Option<(i32, i32)>,
    pub from_chance: bool,
    pub from_inactivity: bool,
    /// Seconds without a player within `min_distance` before the random roll.
    pub inactivity_seconds: i32,
    /// 1-in-N chance per check once inactive.
    pub random_chance: i32,
}

/// Register the `minecraft:despawn` component kind. The inactivity anchor
/// comes and goes with the despawn rules.
pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
) -> Result<(), DefinitionError> {
    builder.register(
        |b: body::Despawn| {
            if b.filters.is_some() {
                return Err(DespawnError::Filters.into());
            }
            Ok(DespawnConfig {
                from_distance: b
                    .despawn_from_distance
                    .map(|d| (d.min_distance, d.max_distance)),
                from_chance: b.despawn_from_chance,
                from_inactivity: b.despawn_from_inactivity,
                inactivity_seconds: b.min_range_inactivity_timer,
                random_chance: b.min_range_random_chance,
            })
        },
        |config: &DespawnConfig, entity| {
            let inactivity = Inactivity::starting_now(entity.world());
            entity.insert((*config, inactivity));
        },
        |entity| {
            entity.remove::<(DespawnConfig, Inactivity)>();
        },
    )
}
