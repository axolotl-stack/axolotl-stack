use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:is_hidden_when_invisible`. Sets that this entity can hide from hostile mobs while invisible.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct IsHiddenWhenInvisible;
