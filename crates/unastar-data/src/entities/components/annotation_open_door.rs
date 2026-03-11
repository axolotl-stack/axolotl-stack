use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:annotation.open_door`. Allows the actor to open doors assuming that that flags set up for the component to use in navigation.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[component(storage = "SparseSet")]
pub struct AnnotationOpenDoor;
