use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:behavior.move_indoors`. Can only be used by Villagers. Allows them to seek shelter indoors.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorMoveIndoors {
    /// priority
    pub priority: Option<i32>,
    /// speed_multiplier
    pub speed_multiplier: Option<f32>,
    /// timeout_cooldown
    pub timeout_cooldown: Option<f32>,
}
impl Default for BehaviorMoveIndoors {
    fn default() -> Self {
        Self {
            priority: None,
            speed_multiplier: Some(0.8f32),
            timeout_cooldown: Some(8f32),
        }
    }
}
