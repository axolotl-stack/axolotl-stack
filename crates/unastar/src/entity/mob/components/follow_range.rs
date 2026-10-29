//! `minecraft:follow_range`: how far a mob pursues its target (read by the
//! target goals in `mob::ai::targeting`).

use bevy_ecs::prelude::*;
use unastar_data::actor_components::body;

use super::common::insert_infallible;
use crate::entity::definition::{ComponentKindsBuilder, DefinitionError};

/// `minecraft:follow_range`: a target goal drops a target farther than
/// `value` blocks, and a walk path search reaches `value` blocks around its
/// goal. `max` only caps the attribute, which nothing raises, so it is not
/// kept.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct FollowRange {
    pub value: f32,
}

/// Register the follow range kind.
pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
) -> Result<(), DefinitionError> {
    insert_infallible(builder, |b: body::FollowRange| FollowRange {
        value: b.value,
    })
}
