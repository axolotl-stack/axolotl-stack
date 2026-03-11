use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:entity_sensor`. A component that fires an event when a set of conditions are met by other entities within the defined range.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[component(storage = "SparseSet")]
pub struct EntitySensor;
