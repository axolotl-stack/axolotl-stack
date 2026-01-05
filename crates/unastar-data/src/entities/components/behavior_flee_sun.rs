use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.flee_sun`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorFleeSun {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
