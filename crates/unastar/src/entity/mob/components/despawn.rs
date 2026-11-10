//! `minecraft:despawn`: the natural despawn rules (run by
//! `mob::life::despawn`).

use std::sync::Arc;

use bevy_ecs::prelude::*;
use thiserror::Error;
use unastar_data::actor_components::body;
use unastar_rules::filter::{BuiltinTest, Filter};

use super::common::{FilterError, filter_with};
use crate::entity::definition::{ComponentError, ComponentKindsBuilder, DefinitionError};
use crate::entity::mob::life::despawn::Inactivity;

/// Why a `minecraft:despawn` body cannot be compiled.
#[derive(Debug, Error)]
pub enum DespawnError {
    #[error(transparent)]
    Filter(#[from] FilterError),
}

impl ComponentError for DespawnError {}

/// `minecraft:despawn`.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct DespawnConfig {
    /// `(min_distance, max_distance)` blocks, when `despawn_from_distance` is present.
    pub from_distance: Option<(i32, i32)>,
    pub from_chance: bool,
    pub from_inactivity: bool,
    /// Seconds without a player within `min_distance` before the random roll.
    pub inactivity_seconds: i32,
    /// 1-in-N chance per check once inactive.
    pub random_chance: i32,
    /// `filters`: when present they replace every other rule; the mob
    /// despawns on the tick they pass (with `random_chance` answered by the
    /// despawn check itself).
    pub filter: Option<Arc<Filter>>,
}

/// Register the `minecraft:despawn` component kind. The inactivity anchor
/// comes and goes with the despawn rules.
pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
) -> Result<(), DefinitionError> {
    builder.register(
        |b: body::Despawn| {
            let filter = b
                .filters
                .as_ref()
                .map(|json| filter_with(json, &[BuiltinTest::RandomChance]))
                .transpose()
                .map_err(DespawnError::from)?
                .map(Arc::new);
            Ok(DespawnConfig {
                from_distance: b
                    .despawn_from_distance
                    .map(|d| (d.min_distance, d.max_distance)),
                from_chance: b.despawn_from_chance,
                from_inactivity: b.despawn_from_inactivity,
                inactivity_seconds: b.min_range_inactivity_timer,
                random_chance: b.min_range_random_chance,
                filter,
            })
        },
        |config: &DespawnConfig, entity| {
            let inactivity = Inactivity::starting_now(entity.world());
            entity.insert((config.clone(), inactivity));
        },
        |entity| {
            entity.remove::<(DespawnConfig, Inactivity)>();
        },
    )
}
