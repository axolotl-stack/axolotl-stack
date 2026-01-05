use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:can_power_jump`
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[component(storage = "SparseSet")]
pub struct CanPowerJump;
