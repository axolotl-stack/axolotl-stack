use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:vibration_listener`. This entity will respond to vibrations.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[component(storage = "SparseSet")]
pub struct VibrationListener;
