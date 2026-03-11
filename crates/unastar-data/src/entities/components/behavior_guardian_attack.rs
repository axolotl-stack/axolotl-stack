use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorGuardianAttackControlFlags {}
impl Default for BehaviorGuardianAttackControlFlags {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorGuardianAttackPriority {}
impl Default for BehaviorGuardianAttackPriority {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.guardian_attack`. Allows this entity to use a laser beam attack. Can only be used by Guardians and Elder Guardians.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorGuardianAttack {
    ///control_flags
    pub control_flags: Option<BehaviorGuardianAttackControlFlags>,
    ///Amount of additional damage dealt from an elder guardian's magic attack.
    pub elder_extra_magic_damage: Option<i32>,
    ///In hard difficulty, amount of additional damage dealt from a guardian's magic attack.
    pub hard_mode_extra_magic_damage: Option<i32>,
    ///Amount of damage dealt from a guardian's magic attack. Magic attack damage is added to the guardian's base attack damage.
    pub magic_damage: Option<i32>,
    ///Guardian attack behavior stops if the target is closer than this distance (doesn't apply to elders).
    pub min_distance: Option<f32>,
    ///priority
    pub priority: Option<BehaviorGuardianAttackPriority>,
    ///Time (in seconds) to wait after starting an attack before playing the guardian attack sound.
    pub sound_delay_time: Option<f32>,
    ///Maximum rotation (in degrees), on the X-axis, this entity can rotate while trying to look at the target.
    pub x_max_rotation: Option<f32>,
    ///Maximum rotation (in degrees), on the Y-axis, this entity can rotate its head while trying to look at the target.
    pub y_max_head_rotation: Option<f32>,
}
impl Default for BehaviorGuardianAttack {
    fn default() -> Self {
        Self {
            control_flags: Some(BehaviorGuardianAttackControlFlags {}),
            elder_extra_magic_damage: Some(2i32),
            hard_mode_extra_magic_damage: Some(2i32),
            magic_damage: Some(1i32),
            min_distance: Some(3f32),
            priority: Some(BehaviorGuardianAttackPriority {}),
            sound_delay_time: Some(0.5f32),
            x_max_rotation: Some(90f32),
            y_max_head_rotation: Some(90f32),
        }
    }
}
