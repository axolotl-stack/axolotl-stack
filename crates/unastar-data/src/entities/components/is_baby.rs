use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:is_baby`. Sets that this entity is a baby.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[component(storage = "SparseSet")]
pub struct IsBaby;
