use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:is_collidable`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct IsCollidable {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
