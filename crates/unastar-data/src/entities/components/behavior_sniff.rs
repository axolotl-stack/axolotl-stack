use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorSniffControlFlags {}
impl Default for BehaviorSniffControlFlags {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorSniffPriority {}
impl Default for BehaviorSniffPriority {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.sniff`. Sniff compels this entity to detect the nearest player within "sniffing_radius" and update its minecraft:suspect_tracking component state.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorSniff {
    ///control_flags
    pub control_flags: Option<BehaviorSniffControlFlags>,
    ///Cooldown range between sniffs in seconds.
    pub cooldown_range: Option<Vec<f32>>,
    ///Sniffing duration in seconds
    pub duration: Option<f32>,
    ///priority
    pub priority: Option<BehaviorSniffPriority>,
    ///Mob detection radius.
    pub sniffing_radius: Option<f32>,
    ///Mob suspicion horizontal radius. When a player is within this radius horizontally, the anger level towards that player is increased.
    pub suspicion_radius_horizontal: Option<f32>,
    ///Mob suspicion vertical radius. When a player is within this radius vertically, the anger level towards that player is increased.
    pub suspicion_radius_vertical: Option<f32>,
}
impl Default for BehaviorSniff {
    fn default() -> Self {
        Self {
            control_flags: Some(BehaviorSniffControlFlags {}),
            cooldown_range: Some(vec![0f32]),
            duration: Some(1f32),
            priority: Some(BehaviorSniffPriority {}),
            sniffing_radius: Some(5f32),
            suspicion_radius_horizontal: Some(3f32),
            suspicion_radius_vertical: Some(3f32),
        }
    }
}
