use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.trade_with_player`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorTradeWithPlayer {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
