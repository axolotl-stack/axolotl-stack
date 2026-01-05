use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.random_hover`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorRandomHover {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
