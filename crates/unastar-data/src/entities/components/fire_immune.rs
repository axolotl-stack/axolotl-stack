use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:fire_immune`
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[component(storage = "SparseSet")]
pub struct FireImmune;
