use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:dimension_bound`. Restricts entities from moving between dimensions when using Minecraft portals, keeping them bound to their current dimension.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[component(storage = "SparseSet")]
pub struct DimensionBound;
