use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:behavior.ranged_attack`. Allows the mob to use ranged attacks like shooting arrows.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorRangedAttack {
    /// attack_interval
    pub attack_interval: Option<f32>,
    /// attack_interval_max
    pub attack_interval_max: Option<f32>,
    /// attack_interval_min
    pub attack_interval_min: Option<f32>,
    /// attack_radius
    pub attack_radius: Option<f32>,
    /// attack_radius_min
    pub attack_radius_min: Option<f32>,
    /// burst_interval
    pub burst_interval: Option<f32>,
    /// burst_shots
    pub burst_shots: Option<i32>,
    /// charge_charged_trigger
    pub charge_charged_trigger: Option<f32>,
    /// charge_shoot_trigger
    pub charge_shoot_trigger: Option<f32>,
    /// priority
    pub priority: Option<i32>,
    /// ranged_fov
    pub ranged_fov: Option<f32>,
    /// set_persistent
    pub set_persistent: Option<bool>,
    /// speed_multiplier
    pub speed_multiplier: Option<f32>,
    /// swing
    pub swing: Option<bool>,
    /// target_in_sight_time
    pub target_in_sight_time: Option<f32>,
    /// x_max_rotation
    pub x_max_rotation: Option<f32>,
    /// y_max_head_rotation
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
            speed_multiplier: Some(1f32),
            swing: Some(false),
            target_in_sight_time: Some(1f32),
            x_max_rotation: Some(30f32),
            y_max_head_rotation: Some(30f32),
        }
    }
}
