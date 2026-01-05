use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.slime_attack`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorSlimeAttack {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
