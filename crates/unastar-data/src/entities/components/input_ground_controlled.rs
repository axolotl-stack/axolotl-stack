use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:input_ground_controlled`. When configured as a rideable entity, the entity will be controlled using WASD controls.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[component(storage = "SparseSet")]
pub struct InputGroundControlled;
