use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorFloatPriority {}
impl Default for BehaviorFloatPriority {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.float`. Allows the mob to stay afloat while swimming.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct BehaviorFloat {
    ///The chance per tick to cause an upward impulse.
    pub chance_per_tick_to_float: Option<f32>,
    ///priority
    pub priority: Option<BehaviorFloatPriority>,
    ///If true, the mob will keep sinking as long as it has passengers.
    pub sink_with_passengers: Option<bool>,
    ///Time in seconds that a floating vehicles head can be underwater before it causes its passengers to dismount.
    pub time_under_water_to_dismount_passengers: Option<f32>,
}
impl Default for BehaviorFloat {
    fn default() -> Self {
        Self {
            chance_per_tick_to_float: Some(0.8f32),
            priority: None,
            sink_with_passengers: Some(false),
            time_under_water_to_dismount_passengers: Some(0f32),
        }
    }
}
