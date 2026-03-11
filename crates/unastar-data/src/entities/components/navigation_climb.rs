use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:navigation.climb`
#[derive(Component, Debug, Clone, Default, PartialEq)]
#[component(storage = "SparseSet")]
pub struct NavigationClimb {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
