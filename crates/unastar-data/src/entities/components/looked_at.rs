use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:looked_at`
#[derive(Component, Debug, Clone, Default, PartialEq)]
#[component(storage = "SparseSet")]
pub struct LookedAt {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
