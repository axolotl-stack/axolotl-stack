use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:is_tamed`. Sets that this entity is currently tamed.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[component(storage = "SparseSet")]
pub struct IsTamed;
