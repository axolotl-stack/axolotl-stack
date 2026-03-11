use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:renders_when_invisible`. When set, the entity will render even when invisible. Appropriate rendering behavior can then be specified in the corresponding "minecraft:client_entity".
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[component(storage = "SparseSet")]
pub struct RendersWhenInvisible;
