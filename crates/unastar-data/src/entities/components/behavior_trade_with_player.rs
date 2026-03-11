use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorTradeWithPlayerPriority {}
impl Default for BehaviorTradeWithPlayerPriority {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.trade_with_player`. Allows the player to trade with this mob.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorTradeWithPlayer {
    ///Conditions that need to be met for the behavior to start.
    pub filters: Option<crate::types::BedrockValue>,
    ///priority
    pub priority: Option<BehaviorTradeWithPlayerPriority>,
}
impl Default for BehaviorTradeWithPlayer {
    fn default() -> Self {
        Self {
            filters: None,
            priority: None,
        }
    }
}
