use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.tempt`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorTempt {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
