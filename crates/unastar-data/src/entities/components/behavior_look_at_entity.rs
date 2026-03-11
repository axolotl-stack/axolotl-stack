use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.look_at_entity`
#[derive(Component, Debug, Clone, Default, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorLookAtEntity {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
