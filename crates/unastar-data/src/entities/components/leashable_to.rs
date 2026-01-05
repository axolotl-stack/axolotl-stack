use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:leashable_to`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct LeashableTo {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
