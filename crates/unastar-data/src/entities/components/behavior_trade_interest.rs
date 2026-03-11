use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:behavior.trade_interest`. Allows the mob to look at a player that is holding a tradable item.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorTradeInterest {
    /// carried_item_switch_time
    pub carried_item_switch_time: Option<f32>,
    /// cooldown
    pub cooldown: Option<f32>,
    /// interest_time
    pub interest_time: Option<f32>,
    /// priority
    pub priority: Option<i32>,
    /// remove_item_time
    pub remove_item_time: Option<f32>,
    /// within_radius
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
