use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorTemptPriority {}
impl Default for BehaviorTemptPriority {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorTemptSpeedMultiplier {}
impl Default for BehaviorTemptSpeedMultiplier {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.tempt`. Allows an entity to be tempted by a set item.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct BehaviorTempt {
    ///If true, the mob can stop being tempted if the player moves too fast while close to this mob.
    pub can_get_scared: Option<bool>,
    ///If true, vertical distance to the player will be considered when tempting.
    pub can_tempt_vertically: Option<bool>,
    ///If true, the mob can be tempted even if it has a passenger (i.e. if being ridden).
    pub can_tempt_while_ridden: Option<bool>,
    ///List of items this mob is tempted by.
    pub items: Option<Vec<crate::types::BedrockValue>>,
    ///Specifies the event to trigger when the goal ends
    pub on_end: Option<crate::types::BedrockValue>,
    ///Specifies the event to trigger when the goal starts
    pub on_start: Option<crate::types::BedrockValue>,
    ///priority
    pub priority: Option<BehaviorTemptPriority>,
    ///Range of random ticks to wait between tempt sounds.
    pub sound_interval: Option<crate::types::BedrockValue>,
    ///speed_multiplier
    pub speed_multiplier: Option<BehaviorTemptSpeedMultiplier>,
    ///The distance at which the mob will stop following the player.
    pub stop_distance: Option<f32>,
    ///Sound to play while the mob is being tempted.
    pub tempt_sound: Option<String>,
    ///Distance in blocks this mob can get tempted by a player holding an item they like.
    pub within_radius: Option<f32>,
}
impl Default for BehaviorTempt {
    fn default() -> Self {
        Self {
            can_get_scared: Some(false),
            can_tempt_vertically: Some(false),
            can_tempt_while_ridden: Some(false),
            items: Some(vec![crate::types::BedrockValue::String("[]".to_string())]),
            on_end: None,
            on_start: None,
            priority: None,
            sound_interval: None,
            speed_multiplier: Some(BehaviorTemptSpeedMultiplier {}),
            stop_distance: Some(1.5f32),
            tempt_sound: None,
            within_radius: Some(0f32),
        }
    }
}
