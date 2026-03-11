use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.go_and_give_items_to_owner`
#[derive(Component, Debug, Clone, Default, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorGoAndGiveItemsToOwner {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
