use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:behavior.ocelotattack`. Can only be used by the Ocelot. Allows it to perform the sneak and pounce attack.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorOcelotattack {
    /// cooldown_time
    pub cooldown_time: Option<f32>,
    /// max_distance
    pub max_distance: Option<f32>,
    /// max_sneak_range
    pub max_sneak_range: Option<f32>,
    /// max_sprint_range
    pub max_sprint_range: Option<f32>,
    /// priority
    pub priority: Option<i32>,
    /// reach_multiplier
    pub reach_multiplier: Option<f32>,
    /// sneak_speed_multiplier
    pub sneak_speed_multiplier: Option<f32>,
    /// sprint_speed_multiplier
    pub sprint_speed_multiplier: Option<f32>,
    /// walk_speed_multiplier
    pub walk_speed_multiplier: Option<f32>,
    /// x_max_rotation
    pub x_max_rotation: Option<f32>,
    /// y_max_head_rotation
    pub y_max_head_rotation: Option<f32>,
}
impl Default for BehaviorOcelotattack {
    fn default() -> Self {
        Self {
            cooldown_time: Some(1f32),
            max_distance: Some(15f32),
            max_sneak_range: Some(15f32),
            max_sprint_range: Some(4f32),
            priority: None,
            reach_multiplier: Some(2f32),
            sneak_speed_multiplier: Some(0.6f32),
            sprint_speed_multiplier: Some(1.33f32),
            walk_speed_multiplier: Some(0.8f32),
            x_max_rotation: Some(30f32),
            y_max_head_rotation: Some(30f32),
        }
    }
}
