use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.admire_item`
#[derive(Component, Debug, Clone, Default, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorAdmireItem {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
