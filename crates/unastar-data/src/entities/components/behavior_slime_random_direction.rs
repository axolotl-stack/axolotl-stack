use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorSlimeRandomDirectionControlFlags {}
impl Default for BehaviorSlimeRandomDirectionControlFlags {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorSlimeRandomDirectionPriority {}
impl Default for BehaviorSlimeRandomDirectionPriority {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.slime_random_direction`. Can only be used by Slimes and Magma Cubes. Allows the mob to move in random directions like a slime.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorSlimeRandomDirection {
    ///Additional time (in whole seconds), chosen randomly in the range of [0, "add_random_time_range"], to add to "min_change_direction_time".
    pub add_random_time_range: Option<i32>,
    ///control_flags
    pub control_flags: Option<BehaviorSlimeRandomDirectionControlFlags>,
    ///Constant minimum time (in seconds) to wait before choosing a new direction.
    pub min_change_direction_time: Option<f32>,
    ///priority
    pub priority: Option<BehaviorSlimeRandomDirectionPriority>,
    ///Maximum rotation angle range (in degrees) when randomly choosing a new direction.
    pub turn_range: Option<i32>,
}
impl Default for BehaviorSlimeRandomDirection {
    fn default() -> Self {
        Self {
            add_random_time_range: Some(3i32),
            control_flags: Some(BehaviorSlimeRandomDirectionControlFlags {}),
            min_change_direction_time: Some(2f32),
            priority: Some(BehaviorSlimeRandomDirectionPriority {}),
            turn_range: Some(360i32),
        }
    }
}
