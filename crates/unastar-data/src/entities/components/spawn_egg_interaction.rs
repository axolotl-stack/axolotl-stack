use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:spawn_egg_interaction`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct SpawnEggInteraction {
    /// Dynamic Bedrock payload for components that are not yet covered by the vendored schema.
    pub data: Option<crate::types::BedrockValue>,
}
