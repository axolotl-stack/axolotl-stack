use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.slime_random_direction`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorSlimeRandomDirection {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
