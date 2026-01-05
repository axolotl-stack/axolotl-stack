use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.transport_items`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorTransportItems {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
