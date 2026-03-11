use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorAvoidBlockControlFlags {}
impl Default for BehaviorAvoidBlockControlFlags {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorAvoidBlockPriority {}
impl Default for BehaviorAvoidBlockPriority {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.avoid_block`. Allows this entity to avoid certain blocks.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorAvoidBlock {
    ///The sound event to play when the mob is avoiding a block.
    pub avoid_block_sound: Option<String>,
    ///control_flags
    pub control_flags: Option<BehaviorAvoidBlockControlFlags>,
    ///Escape trigger.
    pub on_escape: Option<Vec<crate::types::BedrockValue>>,
    ///priority
    pub priority: Option<BehaviorAvoidBlockPriority>,
    ///Maximum distance to look for a block in y.
    pub search_height: Option<i32>,
    ///Maximum distance to look for a block in xz.
    pub search_range: Option<i32>,
    ///The range of time in seconds to randomly wait before playing the sound again.
    pub sound_interval: Option<crate::types::RangeOrVal<f32>>,
    ///Modifier for sprint speed. 1.0 means keep the regular speed, while higher numbers make the sprint speed faster.
    pub sprint_speed_modifier: Option<f32>,
    ///List of block types this mob avoids.
    pub target_blocks: Option<Vec<crate::types::BedrockValue>>,
    ///Block search method.
    pub target_selection_method: Option<String>,
    ///Should start tick interval.
    pub tick_interval: Option<i32>,
    ///Modifier for walking speed. 1.0 means keep the regular speed, while higher numbers make the walking speed faster.
    pub walk_speed_modifier: Option<f32>,
}
impl Default for BehaviorAvoidBlock {
    fn default() -> Self {
        Self {
            avoid_block_sound: Some("undefined".to_string()),
            control_flags: Some(BehaviorAvoidBlockControlFlags {}),
            on_escape: Some(vec![]),
            priority: Some(BehaviorAvoidBlockPriority {}),
            search_height: Some(0i32),
            search_range: Some(0i32),
            sound_interval: Some(crate::types::RangeOrVal::Range {
                min: 3f32,
                max: 8f32,
            }),
            sprint_speed_modifier: Some(1f32),
            target_blocks: Some(vec![]),
            target_selection_method: Some("nearest".to_string()),
            tick_interval: Some(1i32),
            walk_speed_modifier: Some(1f32),
        }
    }
}
