use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:on_death`. Trigger to fire.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct OnDeath {
    ///value
    pub value: crate::types::BedrockValue,
}
impl Default for OnDeath {
    fn default() -> Self {
        Self {
            value: crate::types::BedrockValue::Null,
        }
    }
}
