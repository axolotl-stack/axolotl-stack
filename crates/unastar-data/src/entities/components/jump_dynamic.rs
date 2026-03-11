use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct JumpDynamicFastSkipData {
    ///Duration of the jump animation.
    pub animation_duration: Option<f32>,
    ///The multiplier applied to horizontal velocity when jumping.
    pub distance_scale: Option<f32>,
    ///The force applied vertically when jumping.
    pub height: Option<f32>,
    ///Amount of ticks between sequential jumps.
    pub jump_delay: Option<f32>,
}
impl Default for JumpDynamicFastSkipData {
    fn default() -> Self {
        Self {
            animation_duration: None,
            distance_scale: None,
            height: None,
            jump_delay: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct JumpDynamicRegularSkipData {
    ///Duration of the jump animation.
    pub animation_duration: Option<f32>,
    ///The multiplier applied to horizontal velocity when jumping.
    pub distance_scale: Option<f32>,
    ///The force applied vertically when jumping.
    pub height: Option<f32>,
    ///Amount of ticks between sequential jumps.
    pub jump_delay: Option<f32>,
}
impl Default for JumpDynamicRegularSkipData {
    fn default() -> Self {
        Self {
            animation_duration: None,
            distance_scale: None,
            height: None,
            jump_delay: None,
        }
    }
}
/// Bedrock component `minecraft:jump.dynamic`. Defines a dynamic type jump control that will change jump properties based on the speed modifier of the mob.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct JumpDynamic {
    ///Used when travelling quickly with skip movement
    pub fast_skip_data: Option<JumpDynamicFastSkipData>,
    ///Used during normal skip movement
    pub regular_skip_data: Option<JumpDynamicRegularSkipData>,
}
impl Default for JumpDynamic {
    fn default() -> Self {
        Self {
            fast_skip_data: None,
            regular_skip_data: None,
        }
    }
}
