use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:on_start_landing`. Trigger to fire.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct OnStartLanding {
    ///value
    pub value: crate::types::BedrockValue,
}
impl Default for OnStartLanding {
    fn default() -> Self {
        Self {
            value: crate::types::BedrockValue::Null,
        }
    }
}
