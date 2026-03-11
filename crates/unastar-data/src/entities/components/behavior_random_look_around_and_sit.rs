use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorRandomLookAroundAndSitPriority {}
impl Default for BehaviorRandomLookAroundAndSitPriority {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.random_look_around_and_sit`. Allows the mob to randomly sit and look around for a duration. Note: Must have a sitting animation set up to use this.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorRandomLookAroundAndSit {
    ///If the goal should continue to be used as long as the mob is leashed.
    pub continue_if_leashed: Option<bool>,
    ///The mob will stay sitting on reload.
    pub continue_sitting_on_reload: Option<bool>,
    ///The rightmost angle a mob can look at on the horizontal plane with respect to its initial facing direction.
    pub max_angle_of_view_horizontal: Option<f32>,
    ///The max amount of unique looks a mob will have while looking around.
    pub max_look_count: Option<i32>,
    ///The max amount of time (in ticks) a mob will stay looking at a direction while looking around.
    pub max_look_time: Option<i32>,
    ///The leftmost angle a mob can look at on the horizontal plane with respect to its initial facing direction.
    pub min_angle_of_view_horizontal: Option<f32>,
    ///The min amount of unique looks a mob will have while looking around.
    pub min_look_count: Option<i32>,
    ///The min amount of time (in ticks) a mob will stay looking at a direction while looking around.
    pub min_look_time: Option<i32>,
    ///priority
    pub priority: Option<BehaviorRandomLookAroundAndSitPriority>,
    ///The probability of randomly looking around/sitting.
    pub probability: Option<f32>,
    ///The cooldown in seconds before the goal can be used again.
    pub random_look_around_cooldown: Option<i32>,
}
impl Default for BehaviorRandomLookAroundAndSit {
    fn default() -> Self {
        Self {
            continue_if_leashed: Some(false),
            continue_sitting_on_reload: Some(false),
            max_angle_of_view_horizontal: Some(30f32),
            max_look_count: Some(2i32),
            max_look_time: Some(40i32),
            min_angle_of_view_horizontal: Some(-30f32),
            min_look_count: Some(1i32),
            min_look_time: Some(20i32),
            priority: None,
            probability: Some(0.02f32),
            random_look_around_cooldown: Some(0i32),
        }
    }
}
