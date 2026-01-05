use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.charge_held_item`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorChargeHeldItem {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
