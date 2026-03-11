use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:cannot_be_attacked`. When set, blocks entities from attacking the owner entity unless they have the "minecraft:ignore_cannot_be_attacked" component.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[component(storage = "SparseSet")]
pub struct CannotBeAttacked;
