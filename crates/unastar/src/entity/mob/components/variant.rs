//! `minecraft:variant`: the `VARIANT` actor data (`mob::derive`) and the
//! value `query.variant` reads (`mob::combat::death`).

use bevy_ecs::prelude::*;
use unastar_data::actor_components::body;

use super::common::insert_infallible;
use crate::entity::definition::{ComponentKindsBuilder, DefinitionError};

/// `minecraft:variant`: the mob's variant number.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct VariantConfig {
    pub value: i32,
}

/// Register the variant component kind.
pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
) -> Result<(), DefinitionError> {
    insert_infallible(builder, |b: body::Variant| VariantConfig { value: b.value })
}
