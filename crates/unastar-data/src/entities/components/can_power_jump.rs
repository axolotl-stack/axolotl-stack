use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:can_power_jump`. Allows the entity to power jump like the horse does in vanilla.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[component(storage = "SparseSet")]
pub struct CanPowerJump;
