use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorRandomLookAroundPriority {}
impl Default for BehaviorRandomLookAroundPriority {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.random_look_around`. Allows the mob to randomly look around.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct BehaviorRandomLookAround {
    ///The angle in degrees that an entity can see in the Y-axis (up-down).
    pub angle_of_view_horizontal: Option<i32>,
    ///The angle in degrees that an entity can see in the X-axis (left-right).
    pub angle_of_view_vertical: Option<i32>,
    ///The distance in blocks from which the entity will look at.
    pub look_distance: Option<f32>,
    ///The range of time in seconds the mob will stay looking in a random direction before looking elsewhere
    pub look_time: Option<Vec<f32>>,
    ///priority
    pub priority: Option<BehaviorRandomLookAroundPriority>,
    ///The probability of looking at the target. A value of 1.00 is 100%.
    pub probability: Option<f32>,
}
impl Default for BehaviorRandomLookAround {
    fn default() -> Self {
        Self {
            angle_of_view_horizontal: None,
            angle_of_view_vertical: None,
            look_distance: None,
            look_time: None,
            priority: None,
            probability: None,
        }
    }
}
