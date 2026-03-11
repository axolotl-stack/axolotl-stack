use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorMoveIndoorsPriority {}
impl Default for BehaviorMoveIndoorsPriority {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorMoveIndoorsSpeedMultiplier {}
impl Default for BehaviorMoveIndoorsSpeedMultiplier {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.move_indoors`. Can only be used by Villagers. Allows them to seek shelter indoors.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorMoveIndoors {
    ///priority
    pub priority: Option<BehaviorMoveIndoorsPriority>,
    ///speed_multiplier
    pub speed_multiplier: Option<BehaviorMoveIndoorsSpeedMultiplier>,
    ///The cooldown time in seconds before the goal can be reused after pathfinding fails.
    pub timeout_cooldown: Option<f32>,
}
impl Default for BehaviorMoveIndoors {
    fn default() -> Self {
        Self {
            priority: None,
            speed_multiplier: Some(BehaviorMoveIndoorsSpeedMultiplier {}),
            timeout_cooldown: Some(8f32),
        }
    }
}
