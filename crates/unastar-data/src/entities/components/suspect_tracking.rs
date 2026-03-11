use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:suspect_tracking`. Allows this entity to remember suspicious locations.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[component(storage = "SparseSet")]
pub struct SuspectTracking;
