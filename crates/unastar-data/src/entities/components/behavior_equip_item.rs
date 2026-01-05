use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.equip_item`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorEquipItem {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
