use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:pushable_by_block`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct PushableByBlock {
    /// Dynamic Bedrock payload for components that are not yet covered by the vendored schema.
    pub data: Option<crate::types::BedrockValue>,
}
