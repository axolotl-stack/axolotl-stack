use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:can_fly`. Marks the entity as being able to fly, the pathfinder won't be restricted to paths where a solid block is required underneath it.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct CanFly {
    ///value
    pub value: crate::types::BedrockValue,
}
impl Default for CanFly {
    fn default() -> Self {
        Self {
            value: crate::types::BedrockValue::Null,
        }
    }
}
