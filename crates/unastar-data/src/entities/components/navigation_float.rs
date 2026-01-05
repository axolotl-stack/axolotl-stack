use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:navigation.float`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct NavigationFloat {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
