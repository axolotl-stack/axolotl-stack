use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.wither_target_highest_damage`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorWitherTargetHighestDamage {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
