use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.barter`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorBarter {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
