//! `minecraft:behavior.leap_at_target` (run by `mob::ai::leap`).

use bevy_ecs::prelude::*;
use unastar_data::actor_components::body;

use super::common::insert_infallible;
use super::hostile::controls;
use crate::entity::definition::{ComponentKindsBuilder, DefinitionError};
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
    /// The legacy `target_dist` field (default 0). Kept as authored: the
    /// leap does not depend on it, matching BDS 1.26.51, where the leap
    /// distance window is fixed (`LEAP_MIN_DISTANCE_SQ`..`LEAP_MAX_DISTANCE_SQ`).
    pub target_dist: f32,
}

/// Register the leap goal kind.
pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
) -> Result<(), DefinitionError> {
    insert_infallible(builder, |b: body::BehaviorLeapAtTarget| LeapAtTargetGoal {
        priority: b.priority,
        controls: controls(&b.control_flags, GoalControls::MOVE | GoalControls::JUMP),
        must_be_on_ground: b.must_be_on_ground,
        set_persistent: b.set_persistent,
        yd: b.yd,
        target_dist: b.target_dist.unwrap_or(0.0),
    })
}
