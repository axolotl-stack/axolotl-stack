use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:on_hurt`. Trigger to fire.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct OnHurt {
    ///value
    pub value: crate::types::BedrockValue,
}
impl Default for OnHurt {
    fn default() -> Self {
        Self {
            value: crate::types::BedrockValue::Null,
        }
    }
}
