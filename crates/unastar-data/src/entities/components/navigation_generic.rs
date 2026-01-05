use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:navigation.generic`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct NavigationGeneric {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
