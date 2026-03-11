use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:block_climber`. Allows the player to detect and manuever on the scaffolding block.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[component(storage = "SparseSet")]
pub struct BlockClimber;
