use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorLookAtPlayerPriority {}
impl Default for BehaviorLookAtPlayerPriority {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.look_at_player`. Allows the mob to look at the player when the player is nearby.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct BehaviorLookAtPlayer {
    ///The angle in degrees that the mob can see in the Y-axis (up-down).
    pub angle_of_view_horizontal: Option<i32>,
    ///The angle in degrees that the mob can see in the X-axis (left-right).
    pub angle_of_view_vertical: Option<i32>,
    ///The distance in blocks from which the entity will look at.
    pub look_distance: Option<f32>,
    ///Time range to look at the nearest player.
    pub look_time: Option<crate::types::RangeOrVal<f32>>,
    ///priority
    pub priority: Option<BehaviorLookAtPlayerPriority>,
    ///The probability of looking at the target. A value of 1.00 is 100%.
    pub probability: Option<f32>,
    ///The distance in blocks from which the entity will choose a target.
    pub target_distance: Option<f32>,
}
impl Default for BehaviorLookAtPlayer {
    fn default() -> Self {
        Self {
            angle_of_view_horizontal: Some(360i32),
            angle_of_view_vertical: Some(360i32),
            look_distance: Some(8f32),
            look_time: None,
            priority: None,
            probability: Some(0.02f32),
            target_distance: None,
        }
    }
}
