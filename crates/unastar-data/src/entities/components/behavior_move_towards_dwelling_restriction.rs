use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:behavior.move_towards_dwelling_restriction`. Allows mobs with the dweller component to move toward their Village area that the mob should be restricted to.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorMoveTowardsDwellingRestriction {
    /// priority
    pub priority: Option<i32>,
    /// speed_multiplier
    pub speed_multiplier: Option<f32>,
}
impl Default for BehaviorMoveTowardsDwellingRestriction {
    fn default() -> Self {
        Self {
            priority: Some(0i32),
            speed_multiplier: Some(1f32),
        }
    }
}
