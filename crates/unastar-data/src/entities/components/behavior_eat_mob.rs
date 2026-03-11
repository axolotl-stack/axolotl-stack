use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:behavior.eat_mob`. [EXPERIMENTAL BEHAVIOR] Allows the entity to eat a specified Mob.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorEatMob {
    /// eat_animation_time
    pub eat_animation_time: Option<f32>,
    /// eat_mob_sound
    pub eat_mob_sound: Option<String>,
    /// loot_table
    pub loot_table: Option<String>,
    /// priority
    pub priority: Option<i32>,
    /// pull_in_force
    pub pull_in_force: Option<f32>,
    /// reach_mob_distance
    pub reach_mob_distance: Option<f32>,
    /// run_speed
    pub run_speed: Option<f32>,
}
impl Default for BehaviorEatMob {
    fn default() -> Self {
        Self {
            eat_animation_time: Some(1f32),
            eat_mob_sound: Some("".to_string()),
            loot_table: Some("".to_string()),
            priority: Some(0i32),
            pull_in_force: Some(1f32),
            reach_mob_distance: Some(1f32),
            run_speed: Some(1f32),
        }
    }
}
