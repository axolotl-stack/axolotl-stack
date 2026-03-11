use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:can_climb`. Allows this entity to climb up ladders.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct CanClimb;
