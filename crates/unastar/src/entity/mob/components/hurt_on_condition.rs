//! `minecraft:hurt_on_condition`: damage applied every tick while a filter
//! holds (run by `mob::combat::hurt_on_condition`).

use std::sync::Arc;

use bevy_ecs::prelude::*;
use thiserror::Error;
use unastar_data::actor_components::body::{self, ActorDamageCause};
use unastar_rules::filter::Filter;

use super::common::{FilterError, filter, insert};
use crate::entity::definition::{ComponentError, ComponentKindsBuilder, DefinitionError};

/// Why a `minecraft:hurt_on_condition` body cannot be compiled.
#[derive(Debug, Error)]
pub enum HurtOnConditionError {
    #[error(transparent)]
    Filter(#[from] FilterError),
}

impl ComponentError for HurtOnConditionError {}

/// One `minecraft:hurt_on_condition` row.
#[derive(Debug, Clone, PartialEq)]
pub struct HurtCondition {
    pub filter: Filter,
    pub cause: Option<ActorDamageCause>,
    pub damage_per_tick: i32,
}

/// `minecraft:hurt_on_condition`.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct HurtOnConditionConfig {
    pub conditions: Arc<[HurtCondition]>,
}

/// Register the `minecraft:hurt_on_condition` component kind.
pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
) -> Result<(), DefinitionError> {
    insert(builder, compile)
}

/// Compile a `minecraft:hurt_on_condition` body (mobs through the component
/// registry; players from the vanilla player definition).
pub(crate) fn compile(
    b: body::HurtOnCondition,
) -> Result<HurtOnConditionConfig, HurtOnConditionError> {
    let conditions = b
        .damage_conditions
        .unwrap_or_default()
        .into_iter()
        .map(|row| {
            Ok(HurtCondition {
                filter: match &row.filters {
                    Some(json) => filter(json)?,
                    None => Filter::always(),
                },
                cause: row.cause,
                damage_per_tick: row.damage_per_tick,
            })
        })
        .collect::<Result<Vec<_>, HurtOnConditionError>>()?;
    Ok(HurtOnConditionConfig {
        conditions: conditions.into(),
    })
}
