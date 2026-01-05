use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.pickup_items`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorPickupItems {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
