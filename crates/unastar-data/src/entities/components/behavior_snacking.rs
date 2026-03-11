use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorSnackingPriority {}
impl Default for BehaviorSnackingPriority {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.snacking`. Allows the mob to take a load off and snack on food that it found nearby.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorSnacking {
    ///Items that we are interested in snacking on.
    pub items: Option<Vec<crate::types::BedrockValue>>,
    ///priority
    pub priority: Option<BehaviorSnackingPriority>,
    ///The cooldown time in seconds before the mob is able to snack again.
    pub snacking_cooldown: Option<f32>,
    ///The minimum time in seconds before the mob is able to snack again.
    pub snacking_cooldown_min: Option<f32>,
    ///This is the chance that the mob will stop snacking, from 0 to 1.
    pub snacking_stop_chance: Option<f32>,
}
impl Default for BehaviorSnacking {
    fn default() -> Self {
        Self {
            items: None,
            priority: None,
            snacking_cooldown: Some(7.5f32),
            snacking_cooldown_min: Some(0.5f32),
            snacking_stop_chance: Some(0.0017f32),
        }
    }
}
