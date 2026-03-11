use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:economy_trade_table`
#[derive(Component, Debug, Clone, Default, PartialEq)]
#[component(storage = "SparseSet")]
pub struct EconomyTradeTable {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
