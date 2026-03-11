use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:transient`. An entity with this component will NEVER persist, and forever disappear when unloaded.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[component(storage = "SparseSet")]
pub struct Transient;
