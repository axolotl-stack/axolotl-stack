use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorChargeAttackPriority {}
impl Default for BehaviorChargeAttackPriority {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorChargeAttackSpeedMultiplier {}
impl Default for BehaviorChargeAttackSpeedMultiplier {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.charge_attack`. Allows this entity to damage a target by using a running attack.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorChargeAttack {
    ///A charge attack cannot start if the entity is farther than this distance to the target.
    pub max_distance: Option<f32>,
    ///A charge attack cannot start if the entity is closer than this distance to the target.
    pub min_distance: Option<f32>,
    ///priority
    pub priority: Option<BehaviorChargeAttackPriority>,
    ///speed_multiplier
    pub speed_multiplier: Option<BehaviorChargeAttackSpeedMultiplier>,
    ///Percent chance this entity will start a charge attack, if not already attacking (1.0 = 100%)
    pub success_rate: Option<f32>,
}
impl Default for BehaviorChargeAttack {
    fn default() -> Self {
        Self {
            max_distance: Some(3f32),
            min_distance: Some(2f32),
            priority: Some(BehaviorChargeAttackPriority {}),
            speed_multiplier: Some(BehaviorChargeAttackSpeedMultiplier {}),
            success_rate: Some(0.1428f32),
        }
    }
}
