use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:on_target_acquired`. Trigger to fire.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct OnTargetAcquired {
    ///value
    pub value: crate::types::BedrockValue,
}
impl Default for OnTargetAcquired {
    fn default() -> Self {
        Self {
            value: crate::types::BedrockValue::Null,
        }
    }
}
