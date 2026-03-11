use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorSwoopAttackControlFlags {}
impl Default for BehaviorSwoopAttackControlFlags {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorSwoopAttackPriority {}
impl Default for BehaviorSwoopAttackPriority {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorSwoopAttackSpeedMultiplier {}
impl Default for BehaviorSwoopAttackSpeedMultiplier {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.swoop_attack`. Allows the mob to move to attack a target. The goal ends if it has a horizontal collision or gets hit. Built to be used with flying mobs.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorSwoopAttack {
    ///control_flags
    pub control_flags: Option<BehaviorSwoopAttackControlFlags>,
    ///Added to the base size of the entity, to determine the target's maximum allowable distance, when trying to deal attack damage.
    pub damage_reach: Option<f32>,
    ///Minimum and maximum cooldown time-range (in seconds) between each attempted swoop attack.
    pub delay_range: Option<crate::types::RangeOrVal<f32>>,
    ///priority
    pub priority: Option<BehaviorSwoopAttackPriority>,
    ///speed_multiplier
    pub speed_multiplier: Option<BehaviorSwoopAttackSpeedMultiplier>,
}
impl Default for BehaviorSwoopAttack {
    fn default() -> Self {
        Self {
            control_flags: Some(BehaviorSwoopAttackControlFlags {}),
            damage_reach: Some(0.2f32),
            delay_range: Some(crate::types::RangeOrVal::Range {
                min: 10f32,
                max: 20f32,
            }),
            priority: Some(BehaviorSwoopAttackPriority {}),
            speed_multiplier: Some(BehaviorSwoopAttackSpeedMultiplier {}),
        }
    }
}
