use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:behavior.wither_random_attack_pos_goal`. Allows the wither to launch random attacks. Can only be used by the Wither Boss.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorWitherRandomAttackPosGoal {
    /// priority
    pub priority: Option<i32>,
}
impl Default for BehaviorWitherRandomAttackPosGoal {
    fn default() -> Self {
        Self { priority: None }
    }
}
