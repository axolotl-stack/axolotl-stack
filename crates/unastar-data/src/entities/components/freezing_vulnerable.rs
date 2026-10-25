use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:freezing_vulnerable`
#[derive(Component, Debug, Clone, Default, PartialEq)]
#[component(storage = "SparseSet")]
pub struct FreezingVulnerable {
    /// Dynamic Bedrock payload for components that are not yet covered by the vendored schema.
    pub data: Option<crate::types::BedrockValue>,
}
