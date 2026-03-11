use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:inventory`. Defines this entity's inventory properties.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct Inventory {
    ///Number of slots that this entity can gain per extra strength.
    pub additional_slots_per_strength: Option<i32>,
    ///If true, the contents of this inventory can be removed by a hopper.
    pub can_be_siphoned_from: Option<bool>,
    ///Type of container this entity has. Can be horse, minecart_chest, chest_boat, minecart_hopper, inventory, container or hopper
    pub container_type: Option<String>,
    ///Number of slots the container has.
    pub inventory_size: Option<i32>,
    ///If true, only the entity can access the inventory.
    pub private: Option<bool>,
    ///If true, the entity's inventory can only be accessed by its owner or itself.
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
