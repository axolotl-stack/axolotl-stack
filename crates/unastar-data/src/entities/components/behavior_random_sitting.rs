use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.random_sitting`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorRandomSitting {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
