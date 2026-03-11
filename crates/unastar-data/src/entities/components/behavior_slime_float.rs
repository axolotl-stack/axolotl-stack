use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorSlimeFloatControlFlags {}
impl Default for BehaviorSlimeFloatControlFlags {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorSlimeFloatPriority {}
impl Default for BehaviorSlimeFloatPriority {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorSlimeFloatSpeedMultiplier {}
impl Default for BehaviorSlimeFloatSpeedMultiplier {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.slime_float`. Allow slimes to float in water / lava. Can only be used by Slime and Magma Cubes.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorSlimeFloat {
    ///control_flags
    pub control_flags: Option<BehaviorSlimeFloatControlFlags>,
    ///Percent chance a slime or magma cube has to jump while in water / lava.
    pub jump_chance_percentage: Option<f32>,
    ///priority
    pub priority: Option<BehaviorSlimeFloatPriority>,
    ///speed_multiplier
    pub speed_multiplier: Option<BehaviorSlimeFloatSpeedMultiplier>,
}
impl Default for BehaviorSlimeFloat {
    fn default() -> Self {
        Self {
            control_flags: Some(BehaviorSlimeFloatControlFlags {}),
            jump_chance_percentage: Some(0.8f32),
            priority: Some(BehaviorSlimeFloatPriority {}),
            speed_multiplier: Some(BehaviorSlimeFloatSpeedMultiplier {}),
        }
    }
}
