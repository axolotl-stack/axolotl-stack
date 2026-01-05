use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:navigation.walk`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct NavigationWalk {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
