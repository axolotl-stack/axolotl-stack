use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorEatMobPriority {}
impl Default for BehaviorEatMobPriority {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.eat_mob`. [EXPERIMENTAL BEHAVIOR] Allows the entity to eat a specified Mob.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorEatMob {
    ///Sets the time in seconds the eat animation should play for.
    pub eat_animation_time: Option<f32>,
    ///Sets the sound that should play when eating a mob.
    pub eat_mob_sound: Option<String>,
    ///The loot table for loot to be dropped when eating a mob.
    pub loot_table: Option<String>,
    ///priority
    pub priority: Option<BehaviorEatMobPriority>,
    ///Sets the force which the mob-to-be-eaten is pulled towards the eating mob.
    pub pull_in_force: Option<f32>,
    ///Sets the desired distance to be reached before eating the mob.
    pub reach_mob_distance: Option<f32>,
    ///Sets the entity's speed when running toward the target.
    pub run_speed: Option<f32>,
}
impl Default for BehaviorEatMob {
    fn default() -> Self {
        Self {
            eat_animation_time: Some(1f32),
            eat_mob_sound: Some("".to_string()),
            loot_table: Some("".to_string()),
            priority: Some(BehaviorEatMobPriority {}),
            pull_in_force: Some(1f32),
            reach_mob_distance: Some(1f32),
            run_speed: Some(1f32),
        }
    }
}
