use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:hurt_on_condition`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct HurtOnCondition {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
