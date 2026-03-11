use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorSonicBoomControlFlags {}
impl Default for BehaviorSonicBoomControlFlags {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorSonicBoomPriority {}
impl Default for BehaviorSonicBoomPriority {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorSonicBoomSpeedMultiplier {}
impl Default for BehaviorSonicBoomSpeedMultiplier {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.sonic_boom`. Plays the provided sounds and activates the `SONIC BOOM` actor flag during the specified duration
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorSonicBoom {
    ///Cooldown in seconds required after using this attack until the entity can use sonic boom again.
    pub attack_cooldown: Option<f32>,
    ///Attack damage of the sonic boom.
    pub attack_damage: Option<f32>,
    ///Horizontal range (in blocks) at which the sonic boom can damage the target.
    pub attack_range_horizontal: Option<f32>,
    ///Vertical range (in blocks) at which the sonic boom can damage the target.
    pub attack_range_vertical: Option<f32>,
    ///Sound event for the attack.
    pub attack_sound: Option<String>,
    ///Sound event for the charge up.
    pub charge_sound: Option<String>,
    ///control_flags
    pub control_flags: Option<BehaviorSonicBoomControlFlags>,
    ///Goal duration in seconds.
    pub duration: Option<f32>,
    ///Duration in seconds until the attack sound is played.
    pub duration_until_attack_sound: Option<f32>,
    ///Height cap of the attack knockback's vertical delta.
    pub knockback_height_cap: Option<f32>,
    ///Horizontal strength of the attack's knockback applied to the attack target.
    pub knockback_horizontal_strength: Option<f32>,
    ///Vertical strength of the attack's knockback applied to the attack target.
    pub knockback_vertical_strength: Option<f32>,
    ///priority
    pub priority: Option<BehaviorSonicBoomPriority>,
    ///speed_multiplier
    pub speed_multiplier: Option<BehaviorSonicBoomSpeedMultiplier>,
}
impl Default for BehaviorSonicBoom {
    fn default() -> Self {
        Self {
            attack_cooldown: Some(5f32),
            attack_damage: Some(30f32),
            attack_range_horizontal: Some(15f32),
            attack_range_vertical: Some(20f32),
            attack_sound: Some("".to_string()),
            charge_sound: Some("".to_string()),
            control_flags: Some(BehaviorSonicBoomControlFlags {}),
            duration: Some(0f32),
            duration_until_attack_sound: Some(1.7f32),
            knockback_height_cap: Some(0f32),
            knockback_horizontal_strength: Some(0f32),
            knockback_vertical_strength: Some(0f32),
            priority: Some(BehaviorSonicBoomPriority {}),
            speed_multiplier: Some(BehaviorSonicBoomSpeedMultiplier {}),
        }
    }
}
