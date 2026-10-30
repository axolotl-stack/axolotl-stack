//! `minecraft:behavior.flee_sun` (run by `mob::ai::flee_sun`).

use bevy_ecs::prelude::*;
use unastar_data::actor_components::body;

use super::common::insert_infallible;
use super::hostile::controls;
use crate::entity::definition::{ComponentKindsBuilder, DefinitionError};
use crate::entity::goal_selector::GoalControls;

/// `minecraft:behavior.flee_sun`: a burning mob in daylight walks to cover.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct FleeSunGoal {
    pub priority: i32,
    pub(crate) controls: GoalControls,
    pub speed_multiplier: f32,
}

/// Register the flee-sun goal kind.
pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
) -> Result<(), DefinitionError> {
    insert_infallible(builder, |b: body::BehaviorFleeSun| FleeSunGoal {
        priority: b.priority,
        controls: controls(&b.control_flags, GoalControls::MOVE),
        speed_multiplier: b.speed_multiplier,
    })
}
