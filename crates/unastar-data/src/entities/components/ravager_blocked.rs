use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:ravager_blocked`
#[derive(Component, Debug, Clone, Default, PartialEq)]
#[component(storage = "SparseSet")]
pub struct RavagerBlocked {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
