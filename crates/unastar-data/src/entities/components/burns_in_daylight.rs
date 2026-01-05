use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:burns_in_daylight`
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[component(storage = "SparseSet")]
pub struct BurnsInDaylight;
