use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:behavior.move_through_village`. Can only be used by Villagers. Allows the villagers to create paths around the village.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorMoveThroughVillage {
    /// only_at_night
    pub only_at_night: Option<bool>,
    /// priority
    pub priority: Option<i32>,
    /// speed_multiplier
    pub speed_multiplier: Option<f32>,
}
impl Default for BehaviorMoveThroughVillage {
    fn default() -> Self {
        Self {
            only_at_night: Some(false),
            priority: None,
            speed_multiplier: Some(1f32),
        }
    }
}
