use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:fire_immune`. Sets that this entity doesn't take damage from fire.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[component(storage = "SparseSet")]
pub struct FireImmune;
