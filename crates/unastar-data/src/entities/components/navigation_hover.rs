use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:navigation.hover`
#[derive(Component, Debug, Clone, Default, PartialEq)]
#[component(storage = "SparseSet")]
pub struct NavigationHover {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
