use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:navigation.fly`
#[derive(Component, Debug, Clone, Default, PartialEq)]
#[component(storage = "SparseSet")]
pub struct NavigationFly {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
