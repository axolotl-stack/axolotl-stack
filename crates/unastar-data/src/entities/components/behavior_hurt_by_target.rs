use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.hurt_by_target`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorHurtByTarget {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
