use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:uses_legacy_friction`
#[derive(Component, Debug, Clone, Default, PartialEq)]
#[component(storage = "SparseSet")]
pub struct UsesLegacyFriction {
    /// Dynamic Bedrock payload for components that are not yet covered by the vendored schema.
    pub data: Option<crate::types::BedrockValue>,
}
