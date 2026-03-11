use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:jump.dynamic`
#[derive(Component, Debug, Clone, Default, PartialEq)]
#[component(storage = "SparseSet")]
pub struct JumpDynamic {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
