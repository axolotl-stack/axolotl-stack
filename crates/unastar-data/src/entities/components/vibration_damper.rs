use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:vibration_damper`. Vibrations emitted by this entity will be ignored.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[component(storage = "SparseSet")]
pub struct VibrationDamper;
