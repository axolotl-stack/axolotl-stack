use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:block_movement_slowdown_immunity`
#[derive(Component, Debug, Clone, Default, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BlockMovementSlowdownImmunity {
    /// Dynamic Bedrock payload for components that are not yet covered by the vendored schema.
    pub data: Option<crate::types::BedrockValue>,
}
