use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorSlimeAttackControlFlags {}
impl Default for BehaviorSlimeAttackControlFlags {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorSlimeAttackPriority {}
impl Default for BehaviorSlimeAttackPriority {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorSlimeAttackSpeedMultiplier {}
impl Default for BehaviorSlimeAttackSpeedMultiplier {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.slime_attack`. Can only be used by Slimes and Magma Cubes. Allows the mob to use a melee attack like the slime's.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorSlimeAttack {
    ///control_flags
    pub control_flags: Option<BehaviorSlimeAttackControlFlags>,
    ///UNDOCUMENTED
    pub grow_tired_cooldown_time: Option<f32>,
    ///priority
    pub priority: Option<BehaviorSlimeAttackPriority>,
    ///Allows the actor to be set to persist upon targeting a player.
    pub set_persistent: Option<bool>,
    ///speed_multiplier
    pub speed_multiplier: Option<BehaviorSlimeAttackSpeedMultiplier>,
    ///Maximum rotation (in degrees), on the X-axis, this entity can rotate while trying to look at the target.
    pub x_max_rotation: Option<f32>,
    ///Maximum rotation (in degrees), on the Y-axis, this entity can rotate while trying to look at the target.
    pub y_max_rotation: Option<f32>,
}
impl Default for BehaviorSlimeAttack {
    fn default() -> Self {
        Self {
            control_flags: Some(BehaviorSlimeAttackControlFlags {}),
            grow_tired_cooldown_time: Some(15f32),
            priority: Some(BehaviorSlimeAttackPriority {}),
            set_persistent: Some(false),
            speed_multiplier: Some(BehaviorSlimeAttackSpeedMultiplier {}),
            x_max_rotation: Some(10f32),
            y_max_rotation: Some(10f32),
        }
    }
}
