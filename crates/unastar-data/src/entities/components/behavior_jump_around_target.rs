use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorJumpAroundTargetPriority {}
impl Default for BehaviorJumpAroundTargetPriority {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.jump_around_target`. Allows an entity to jump around a target.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorJumpAroundTarget {
    ///Enables collision checks when calculating the jump. Setting check_collision to true may affect performance and should be used with care.
    pub check_collision: Option<bool>,
    ///Scaling temporarily applied to the entity's AABB bounds when jumping. A smaller bounding box reduces the risk of collisions during the jump. When check_collision is true it also increases the chance of being able to jump when close to obstacles.
    pub entity_bounding_box_scale: Option<crate::types::BedrockValue>,
    ///Conditions that need to be met for the behavior to start.
    pub filters: Option<crate::types::BedrockValue>,
    ///The jump angles in float degrees that are allowed when performing the jump. The order in which the angles are chosen is randomized.
    pub jump_angles: Option<Vec<f32>>,
    ///The time in seconds to spend in cooldown before this goal can be used again.
    pub jump_cooldown_duration: Option<f32>,
    ///The time in seconds to spend in cooldown after being hurt before this goal can be used again.
    pub jump_cooldown_when_hurt_duration: Option<f32>,
    ///The range deciding how close to and how far away from the target the landing position can be when jumping.
    pub landing_distance_from_target: Option<Vec<f32>>,
    ///This angle (in degrees) is used for controlling the spread when picking a landing position behind the target. A zero spread angle means the landing position will be straight behind the target with no variance. A 90 degree spread angle means the landing position can be up to 45 degrees to the left and to the right of the position straight behind the target's view direction.
    pub landing_position_spread_degrees: Option<i32>,
    ///If the entity was hurt within these last seconds, the jump_cooldown_when_hurt_duration will be used instead of jump_cooldown_duration.
    pub last_hurt_duration: Option<f32>,
    ///If the entity's line of sight towards its target is obstructed by an obstacle with a height below this number, the obstacle will be ignored, and the goal will try to find a valid landing position.
    pub line_of_sight_obstruction_height_ignore: Option<i32>,
    ///Maximum velocity a jump can be performed at.
    pub max_jump_velocity: Option<f32>,
    ///The time in seconds to spend preparing for the jump.
    pub prepare_jump_duration: Option<f32>,
    ///priority
    pub priority: Option<BehaviorJumpAroundTargetPriority>,
    ///The number of blocks above the entity's head that has to be air for this goal to be usable.
    pub required_vertical_space: Option<i32>,
    ///The number of blocks above and below from the jump target position that will be checked to find a surface to land on.
    pub snap_to_surface_block_range: Option<i32>,
    ///Target needs to be within this range for the jump to happen.
    pub valid_distance_to_target: Option<Vec<f32>>,
}
impl Default for BehaviorJumpAroundTarget {
    fn default() -> Self {
        Self {
            check_collision: Some(false),
            entity_bounding_box_scale: Some(crate::types::BedrockValue::Float(0.7f64)),
            filters: None,
            jump_angles: Some(vec![0f32]),
            jump_cooldown_duration: Some(0.5f32),
            jump_cooldown_when_hurt_duration: Some(0.1f32),
            landing_distance_from_target: None,
            landing_position_spread_degrees: Some(90i32),
            last_hurt_duration: Some(2f32),
            line_of_sight_obstruction_height_ignore: Some(4i32),
            max_jump_velocity: Some(1.4f32),
            prepare_jump_duration: Some(0.5f32),
            priority: None,
            required_vertical_space: Some(4i32),
            snap_to_surface_block_range: Some(10i32),
            valid_distance_to_target: None,
        }
    }
}
