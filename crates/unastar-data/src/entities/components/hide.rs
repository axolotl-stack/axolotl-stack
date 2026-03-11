use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:hide`. Compels an entity to move to and hide at their owned POI or the closest nearby.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[component(storage = "SparseSet")]
pub struct Hide;
