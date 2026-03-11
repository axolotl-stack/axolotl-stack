use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:can_fly`
#[derive(Component, Debug, Clone, Default, PartialEq)]
#[component(storage = "SparseSet")]
pub struct CanFly {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
