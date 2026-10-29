//! `minecraft:is_sheared`: the actor shows its sheared model.
//!
//! Shearing itself is data: the sheep's `minecraft:interact` row (shears,
//! `hurt_item` 1, `spawn_items` wool of the sheep's colour, event
//! `minecraft:on_sheared`) swaps in the group carrying `is_sheared` and drops
//! `is_dyeable`, which the row's filter requires, so a sheared sheep cannot
//! be sheared again until eating grass brings both back. This component's
//! only server-side effect is the `SHEARED` actor flag viewers draw.

use jolyne::valentine::actor_flags;
use unastar_data::actor_components::body;

use crate::entity::definition::flags::register_flag;
use crate::entity::definition::{ComponentKindsBuilder, DefinitionError};

/// Register the `is_sheared` component kind.
pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
) -> Result<(), DefinitionError> {
    register_flag::<body::IsSheared, { actor_flags::SHEARED }>(builder)
}
