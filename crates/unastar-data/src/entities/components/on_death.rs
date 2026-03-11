use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:on_death`
#[derive(Component, Debug, Clone, Default, PartialEq)]
#[component(storage = "SparseSet")]
pub struct OnDeath {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
