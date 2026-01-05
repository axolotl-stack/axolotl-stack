use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:can_fly`
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[component(storage = "SparseSet")]
pub struct CanFly;
