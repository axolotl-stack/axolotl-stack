//! `minecraft:variable_max_auto_step`: how high the mob steps up without
//! jumping (applied to the physics body by `mob::derive`).

use bevy_ecs::prelude::*;
use unastar_data::actor_components::body;

use super::common::insert_infallible;
use crate::entity::definition::{ComponentKindsBuilder, DefinitionError};

/// `minecraft:variable_max_auto_step`. Only `base_value` acts:
/// `controlled_value` applies while a player rides the mob (riding is not
/// implemented) and `jump_prevented_value` on blocks that prevent jumping,
/// which the physics step does not model.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct AutoStepConfig {
    pub base_value: f32,
}

/// Register the auto-step kind.
pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
) -> Result<(), DefinitionError> {
    insert_infallible(builder, |b: body::VariableMaxAutoStep| AutoStepConfig {
        base_value: b.base_value,
    })
}
