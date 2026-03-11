use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:on_target_escape`. Trigger to fire.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct OnTargetEscape {
    ///value
    pub value: crate::types::BedrockValue,
}
impl Default for OnTargetEscape {
    fn default() -> Self {
        Self {
            value: crate::types::BedrockValue::Null,
        }
    }
}
