use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:can_join_raid`. Sets that this entity can join a raid.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[component(storage = "SparseSet")]
pub struct CanJoinRaid;
