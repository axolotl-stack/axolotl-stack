use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:movement.fly`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct MovementFly {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
