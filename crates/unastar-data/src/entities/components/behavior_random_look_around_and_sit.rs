use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:behavior.random_look_around_and_sit`. Allows the mob to randomly sit and look around for a duration. Note: Must have a sitting animation set up to use this.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorRandomLookAroundAndSit {
    /// continue_if_leashed
    pub continue_if_leashed: Option<bool>,
    /// continue_sitting_on_reload
    pub continue_sitting_on_reload: Option<bool>,
    /// max_angle_of_view_horizontal
    pub max_angle_of_view_horizontal: Option<f32>,
    /// max_look_count
    pub max_look_count: Option<i32>,
    /// max_look_time
    pub max_look_time: Option<i32>,
    /// min_angle_of_view_horizontal
    pub min_angle_of_view_horizontal: Option<f32>,
    /// min_look_count
    pub min_look_count: Option<i32>,
    /// min_look_time
    pub min_look_time: Option<i32>,
    /// priority
    pub priority: Option<i32>,
    /// probability
    pub probability: Option<f32>,
    /// random_look_around_cooldown
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
