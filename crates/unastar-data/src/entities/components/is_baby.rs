use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:is_baby`
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[component(storage = "SparseSet")]
pub struct IsBaby;
