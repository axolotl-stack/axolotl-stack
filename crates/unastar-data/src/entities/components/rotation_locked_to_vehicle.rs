use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:rotation_locked_to_vehicle`. Aligns both the entity's body rotation and its overall rotation with that of its mounted vehicle
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[component(storage = "SparseSet")]
pub struct RotationLockedToVehicle;
