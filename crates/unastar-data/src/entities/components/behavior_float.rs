use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:behavior.float`. Allows the mob to stay afloat while swimming.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct BehaviorFloat {
    /// chance_per_tick_to_float
    pub chance_per_tick_to_float: Option<f32>,
    /// priority
    pub priority: Option<i32>,
    /// sink_with_passengers
    pub sink_with_passengers: Option<bool>,
    /// time_under_water_to_dismount_passengers
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
