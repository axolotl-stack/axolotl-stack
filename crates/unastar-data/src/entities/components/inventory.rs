use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:inventory`
#[derive(Component, Debug, Clone, PartialEq)]
pub struct Inventory {
    /// inventory_size
    pub size: i32,
    /// container_type
    pub container_type: Option<String>,
    /// can_be_siphoned_from
    pub can_be_siphoned_from: bool,
    /// private
    pub private: bool,
}
impl Default for Inventory {
    fn default() -> Self {
        Self {
            size: 5i32,
            container_type: None,
            can_be_siphoned_from: false,
            private: false,
        }
    }
}
