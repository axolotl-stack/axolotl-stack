use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:inventory`. Defines this entity's inventory properties.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct Inventory {
    /// additional_slots_per_strength
    pub additional_slots_per_strength: Option<i32>,
    /// can_be_siphoned_from
    pub can_be_siphoned_from: Option<bool>,
    /// container_type
    pub container_type: Option<String>,
    /// inventory_size
    pub inventory_size: Option<i32>,
    /// private
    pub private: Option<bool>,
    /// restrict_to_owner
    pub restrict_to_owner: Option<bool>,
}
impl Default for Inventory {
    fn default() -> Self {
        Self {
            additional_slots_per_strength: Some(0i32),
            can_be_siphoned_from: Some(false),
            container_type: Some("none".to_string()),
            inventory_size: Some(5i32),
            private: Some(false),
            restrict_to_owner: Some(false),
        }
    }
}
