use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorSwimUpForBreathControlFlags {}
impl Default for BehaviorSwimUpForBreathControlFlags {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorSwimUpForBreathPriority {}
impl Default for BehaviorSwimUpForBreathPriority {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.swim_up_for_breath`. Allows the mob to try to move to air once it is close to running out of its total breathable supply.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorSwimUpForBreath {
    ///control_flags
    pub control_flags: Option<BehaviorSwimUpForBreathControlFlags>,
    ///The material the mob is traveling in. An air block will only be considered valid to move to with a block of this material below it.
    pub material_type: Option<String>,
    ///priority
    pub priority: Option<BehaviorSwimUpForBreathPriority>,
    ///The height (in blocks) above the mob's current position that it will search for a valid air block to move to. If a valid block cannot be found, the mob will move to the position this many blocks above it.
    pub search_height: Option<i32>,
    ///The radius (in blocks) around the mob's current position that it will search for a valid air block to move to.
    pub search_radius: Option<i32>,
    ///Movement speed multiplier of the mob when using this Goal.
    pub speed_mod: Option<f32>,
}
impl Default for BehaviorSwimUpForBreath {
    fn default() -> Self {
        Self {
            control_flags: Some(BehaviorSwimUpForBreathControlFlags {}),
            material_type: Some("water".to_string()),
            priority: Some(BehaviorSwimUpForBreathPriority {}),
            search_height: Some(16i32),
            search_radius: Some(4i32),
            speed_mod: Some(1.4f32),
        }
    }
}
