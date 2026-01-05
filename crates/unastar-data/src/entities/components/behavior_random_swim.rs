use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.random_swim`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorRandomSwim {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
