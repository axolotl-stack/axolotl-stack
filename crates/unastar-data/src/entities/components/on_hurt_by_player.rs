use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:on_hurt_by_player`. Trigger to fire.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct OnHurtByPlayer {
    ///value
    pub value: crate::types::BedrockValue,
}
impl Default for OnHurtByPlayer {
    fn default() -> Self {
        Self {
            value: crate::types::BedrockValue::Null,
        }
    }
}
