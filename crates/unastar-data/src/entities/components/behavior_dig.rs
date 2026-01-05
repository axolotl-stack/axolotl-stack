use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.dig`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorDig {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
