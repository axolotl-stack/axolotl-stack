use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:mob_effect`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct MobEffect {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
