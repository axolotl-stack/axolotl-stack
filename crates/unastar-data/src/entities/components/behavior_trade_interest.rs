use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorTradeInterestPriority {}
impl Default for BehaviorTradeInterestPriority {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.trade_interest`. Allows the mob to look at a player that is holding a tradable item.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorTradeInterest {
    ///The Maximum time in seconds that the trader will hold an item before attempting to switch for a different item that takes the same trade.
    pub carried_item_switch_time: Option<f32>,
    ///The time in seconds before the trader can use this goal again.
    pub cooldown: Option<f32>,
    ///The Maximum time in seconds that the trader will be interested with showing it's trade items.
    pub interest_time: Option<f32>,
    ///priority
    pub priority: Option<BehaviorTradeInterestPriority>,
    ///The Maximum time in seconds that the trader will wait when you no longer have items to trade.
    pub remove_item_time: Option<f32>,
    ///Distance in blocks this mob can be interested by a player holding an item they like.
    pub within_radius: Option<f32>,
}
impl Default for BehaviorTradeInterest {
    fn default() -> Self {
        Self {
            carried_item_switch_time: Some(2f32),
            cooldown: Some(2f32),
            interest_time: Some(45f32),
            priority: None,
            remove_item_time: Some(1f32),
            within_radius: Some(0f32),
        }
    }
}
