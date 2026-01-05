use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:is_tamed`
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[component(storage = "SparseSet")]
pub struct IsTamed;
