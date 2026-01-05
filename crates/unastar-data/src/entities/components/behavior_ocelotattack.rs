use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.ocelotattack`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorOcelotattack {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
