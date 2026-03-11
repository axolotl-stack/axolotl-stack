use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorFireAtTargetPriority {}
impl Default for BehaviorFireAtTargetPriority {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.fire_at_target`. Allows an entity to attack by firing a shot with a delay. Anchor and offset parameters of this component overrides the anchor and offset from projectile component.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorFireAtTarget {
    ///The cooldown time in seconds before this goal can be used again.
    pub attack_cooldown: Option<f32>,
    ///Target needs to be within this range for the attack to happen.
    pub attack_range: Option<Vec<f32>>,
    ///Conditions that need to be met for the behavior to start.
    pub filters: Option<crate::types::BedrockValue>,
    ///Maximum head rotation (in degrees), on the X-axis, that this entity can apply while trying to look at the target.
    pub max_head_rotation_x: Option<f32>,
    ///Maximum head rotation (in degrees), on the Y-axis, that this entity can apply while trying to look at the target.
    pub max_head_rotation_y: Option<f32>,
    ///Entity anchor for the projectile spawn location.
    pub owner_anchor: Option<i32>,
    ///Offset vector from the owner_anchor.
    pub owner_offset: Option<Vec<f32>>,
    ///Time in seconds between firing the projectile and ending the goal.
    pub post_shoot_delay: Option<f32>,
    ///Time in seconds before firing the projectile.
    pub pre_shoot_delay: Option<f32>,
    ///priority
    pub priority: Option<BehaviorFireAtTargetPriority>,
    ///Actor definition to use as projectile for the ranged attack. The actor must be a projectile.
    pub projectile_def: Option<String>,
    ///Field of view (in degrees) when using sensing to detect a target for attack.
    pub ranged_fov: Option<f32>,
    ///Entity anchor for projectile target.
    pub target_anchor: Option<i32>,
    ///Offset vector from the target_anchor.
    pub target_offset: Option<Vec<f32>>,
}
impl Default for BehaviorFireAtTarget {
    fn default() -> Self {
        Self {
            attack_cooldown: Some(0.5f32),
            attack_range: None,
            filters: None,
            max_head_rotation_x: Some(30f32),
            max_head_rotation_y: Some(30f32),
            owner_anchor: Some(2i32),
            owner_offset: Some(vec![0f32, 0f32, 0f32]),
            post_shoot_delay: Some(0.2f32),
            pre_shoot_delay: Some(0.75f32),
            priority: None,
            projectile_def: None,
            ranged_fov: Some(90f32),
            target_anchor: Some(2i32),
            target_offset: Some(vec![0f32, 0f32, 0f32]),
        }
    }
}
