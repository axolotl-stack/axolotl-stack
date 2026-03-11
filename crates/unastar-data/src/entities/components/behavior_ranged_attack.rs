use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorRangedAttackPriority {}
impl Default for BehaviorRangedAttackPriority {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorRangedAttackSpeedMultiplier {}
impl Default for BehaviorRangedAttackSpeedMultiplier {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.ranged_attack`. Allows the mob to use ranged attacks like shooting arrows.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorRangedAttack {
    ///Alternative to "attack_interval_min" & "attack_interval_max". Consistent reload-time (in seconds), when not using a charged shot. Does not scale with target-distance.
    pub attack_interval: Option<f32>,
    ///Maximum bound for reload-time range (in seconds), when not using a charged shot. Reload-time range scales with target-distance.
    pub attack_interval_max: Option<f32>,
    ///Minimum bound for reload-time range (in seconds), when not using a charged shot. Reload-time range scales with target-distance.
    pub attack_interval_min: Option<f32>,
    ///Minimum distance to target before this entity will attempt to shoot.
    pub attack_radius: Option<f32>,
    ///Minimum distance the target can be for this mob to fire. If the target is closer, this mob will move first before firing
    pub attack_radius_min: Option<f32>,
    ///Time (in seconds) between each individual shot when firing a burst of shots from a charged up attack.
    pub burst_interval: Option<f32>,
    ///Number of shots fired every time the attacking entity uses a charged up attack.
    pub burst_shots: Option<i32>,
    ///Time (in seconds, then add "charge_shoot_trigger"), before a charged up attack is done charging. Charge-time decays while target is not in sight.
    pub charge_charged_trigger: Option<f32>,
    ///Amount of time (in seconds, then doubled) a charged shot must be charging before reloading burst shots. Charge-time decays while target is not in sight.
    pub charge_shoot_trigger: Option<f32>,
    ///priority
    pub priority: Option<BehaviorRangedAttackPriority>,
    ///Field of view (in degrees) when using sensing to detect a target for attack.
    pub ranged_fov: Option<f32>,
    ///Allows the actor to be set to persist upon targeting a player.
    pub set_persistent: Option<bool>,
    ///speed_multiplier
    pub speed_multiplier: Option<BehaviorRangedAttackSpeedMultiplier>,
    ///If a swing animation (using variable.attack_time) exists, this causes the actor to swing their arm(s) upon firing the ranged attack.
    pub swing: Option<bool>,
    ///Minimum amount of time (in seconds) the attacking entity needs to see the target before moving toward it.
    pub target_in_sight_time: Option<f32>,
    ///Maximum rotation (in degrees), on the X-axis, this entity can rotate while trying to look at the target.
    pub x_max_rotation: Option<f32>,
    ///Maximum rotation (in degrees), on the Y-axis, this entity can rotate its head while trying to look at the target.
    pub y_max_head_rotation: Option<f32>,
}
impl Default for BehaviorRangedAttack {
    fn default() -> Self {
        Self {
            attack_interval: Some(0f32),
            attack_interval_max: Some(0f32),
            attack_interval_min: Some(0f32),
            attack_radius: Some(0f32),
            attack_radius_min: Some(0f32),
            burst_interval: Some(0f32),
            burst_shots: Some(1i32),
            charge_charged_trigger: Some(0f32),
            charge_shoot_trigger: Some(0f32),
            priority: None,
            ranged_fov: Some(90f32),
            set_persistent: Some(false),
            speed_multiplier: Some(BehaviorRangedAttackSpeedMultiplier {}),
            swing: Some(false),
            target_in_sight_time: Some(1f32),
            x_max_rotation: Some(30f32),
            y_max_head_rotation: Some(30f32),
        }
    }
}
