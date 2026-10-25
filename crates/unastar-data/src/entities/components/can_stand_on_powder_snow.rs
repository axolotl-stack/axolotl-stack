use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:can_stand_on_powder_snow`
#[derive(Component, Debug, Clone, Default, PartialEq)]
#[component(storage = "SparseSet")]
pub struct CanStandOnPowderSnow {
    /// Dynamic Bedrock payload for components that are not yet covered by the vendored schema.
    pub data: Option<crate::types::BedrockValue>,
}
