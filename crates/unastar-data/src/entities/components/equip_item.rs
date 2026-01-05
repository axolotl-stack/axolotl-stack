use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:equip_item`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct EquipItem {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
