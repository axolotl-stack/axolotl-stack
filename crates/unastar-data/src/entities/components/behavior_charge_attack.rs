use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:behavior.charge_attack`. Allows this entity to damage a target by using a running attack.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorChargeAttack {
    /// max_distance
    pub max_distance: Option<f32>,
    /// min_distance
    pub min_distance: Option<f32>,
    /// priority
    pub priority: Option<i32>,
    /// speed_multiplier
    pub speed_multiplier: Option<f32>,
    /// success_rate
    pub success_rate: Option<f32>,
}
impl Default for BehaviorChargeAttack {
    fn default() -> Self {
        Self {
            max_distance: Some(3f32),
            min_distance: Some(2f32),
            priority: Some(0i32),
            speed_multiplier: Some(1f32),
            success_rate: Some(0.1428f32),
        }
    }
}
