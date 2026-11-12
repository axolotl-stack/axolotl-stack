//! `minecraft:behavior.leap_at_target` (run by `mob::ai::leap`).

use bevy_ecs::prelude::*;
use thiserror::Error;
use unastar_data::actor_components::body;

use super::common::insert;
use super::hostile::controls;
use crate::entity::definition::{ComponentError, ComponentKindsBuilder, DefinitionError};
use crate::entity::goal_selector::GoalControls;

/// `minecraft:behavior.leap_at_target`: jump at the target.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct LeapAtTargetGoal {
    pub priority: i32,
    pub(crate) controls: GoalControls,
    /// Only leap while standing on the ground.
    pub must_be_on_ground: bool,
    /// Leaping makes the mob persistent.
    pub set_persistent: bool,
    /// The leap's vertical velocity.
    pub yd: f32,
}

/// A leap goal authoring what no vanilla mob does.
#[derive(Debug, Error)]
pub enum LeapError {
    /// The legacy `target_dist` (only the ocelot authors it); its effect on
    /// the leap is not measured.
    #[error("behavior.leap_at_target: target_dist is not supported")]
    TargetDist,
}

impl ComponentError for LeapError {}

/// Register the leap goal kind.
pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
) -> Result<(), DefinitionError> {
    insert(builder, |b: body::BehaviorLeapAtTarget| {
        if b.target_dist.is_some() {
            return Err(LeapError::TargetDist);
        }
        Ok(LeapAtTargetGoal {
            priority: b.priority,
            controls: controls(&b.control_flags, GoalControls::MOVE | GoalControls::JUMP),
            must_be_on_ground: b.must_be_on_ground,
            set_persistent: b.set_persistent,
            yd: b.yd,
        })
    })
}
