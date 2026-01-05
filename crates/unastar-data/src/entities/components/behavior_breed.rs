use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.breed`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorBreed {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
