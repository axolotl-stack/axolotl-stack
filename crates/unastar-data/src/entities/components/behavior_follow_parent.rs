use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:behavior.follow_parent`. Allows the mob to follow their parent around.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorFollowParent {
    /// priority
    pub priority: Option<i32>,
    /// speed_multiplier
    pub speed_multiplier: Option<f32>,
}
impl Default for BehaviorFollowParent {
    fn default() -> Self {
        Self {
            priority: None,
            speed_multiplier: Some(1f32),
        }
    }
}
