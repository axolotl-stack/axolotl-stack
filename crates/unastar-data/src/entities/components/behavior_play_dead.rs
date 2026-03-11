use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorPlayDeadPriority {}
impl Default for BehaviorPlayDeadPriority {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.play_dead`. Allows the mob to play dead when attacked by other entities. When playing dead, other entities will not target this mob.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorPlayDead {
    ///Whether the mob will receive the regeneration effect while playing dead.
    pub apply_regeneration: Option<bool>,
    ///The list of Entity Damage Sources that will cause this mob to play dead.
    pub damage_sources: Option<Vec<String>>,
    ///The amount of time the mob will remain playing dead (in seconds).
    pub duration: Option<f32>,
    ///The list of other triggers that are required for the mob to activate play dead.
    pub filters: Option<crate::types::BedrockValue>,
    ///The amount of health at which damage will cause the mob to play dead.
    pub force_below_health: Option<i32>,
    ///priority
    pub priority: Option<BehaviorPlayDeadPriority>,
    ///The range of damage that may cause the goal to start depending on randomness. Damage taken below the min will never cause the goal to start. Damage taken above the max will always cause the goal to start.
    pub random_damage_range: Option<Vec<i32>>,
    ///The likelihood of this goal starting upon taking damage.
    pub random_start_chance: Option<f32>,
}
impl Default for BehaviorPlayDead {
    fn default() -> Self {
        Self {
            apply_regeneration: Some(false),
            damage_sources: Some(vec!["all".to_string()]),
            duration: Some(1f32),
            filters: None,
            force_below_health: Some(0i32),
            priority: None,
            random_damage_range: None,
            random_start_chance: Some(1f32),
        }
    }
}
