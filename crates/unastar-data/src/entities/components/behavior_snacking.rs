use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.snacking`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorSnacking {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
