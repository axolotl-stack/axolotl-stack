use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorMoveAroundTargetPriority {}
impl Default for BehaviorMoveAroundTargetPriority {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.move_around_target`. Allows an entity to move around a target. If the entity is too close (i.e. closer than destination range min and height difference limit) it will try to move away from its target. If the entity is too far away from its target it will try to move closer to a random position within the destination range. A randomized amount of those positions will be behind the target, and the spread can be tweaked with 'destination_pos_search_spread_degrees'.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorMoveAroundTarget {
    ///This angle (in degrees) is used for controlling the spread when picking a destination position behind the target. A zero spread angle means the destination position will be straight behind the target with no variance. A 90 degree spread angle means the destination position can be up to 45 degrees to the left and to the right of the position straight behind the target's view direction.
    pub destination_pos_search_spread_degrees: Option<f32>,
    ///UNDOCUMENTED
    pub destination_pos_spread_degrees: Option<f32>,
    ///The range of distances from the target entity within which the goal should look for a position to move the owner entity to.
    pub destination_position_range: Option<Vec<f32>>,
    ///Conditions that need to be met for the behavior to start.
    pub filters: Option<crate::types::BedrockValue>,
    ///Distance in height (in blocks) between the owner entity and the target has to be less than this value when owner checks if it is too close and should move away from the target. This value needs to be bigger than zero for the move away logic to trigger.
    pub height_difference_limit: Option<f32>,
    ///Horizontal search distance (in blocks) when searching for a position to move away from target.
    pub horizontal_search_distance: Option<i32>,
    ///The speed with which the entity should move to its target position.
    pub movement_speed: Option<f32>,
    ///priority
    pub priority: Option<BehaviorMoveAroundTargetPriority>,
    ///Number of ticks needed to complete a stay at the block.
    pub vertical_search_distance: Option<i32>,
}
impl Default for BehaviorMoveAroundTarget {
    fn default() -> Self {
        Self {
            destination_pos_search_spread_degrees: None,
            destination_pos_spread_degrees: Some(90f32),
            destination_position_range: None,
            filters: None,
            height_difference_limit: Some(10f32),
            horizontal_search_distance: Some(5i32),
            movement_speed: Some(0.6f32),
            priority: None,
            vertical_search_distance: Some(5i32),
        }
    }
}
