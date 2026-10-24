use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:on_equipment_changed`
#[derive(Component, Debug, Clone, Default, PartialEq)]
#[component(storage = "SparseSet")]
pub struct OnEquipmentChanged {
    /// Dynamic Bedrock payload for components that are not yet covered by the vendored schema.
    pub data: Option<crate::types::BedrockValue>,
}
