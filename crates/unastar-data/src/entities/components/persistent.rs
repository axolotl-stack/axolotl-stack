use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:persistent`. Defines whether an entity should be persistent in the game world.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[component(storage = "SparseSet")]
pub struct Persistent;
