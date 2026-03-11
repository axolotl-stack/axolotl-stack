use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorAdmireItemPriority {}
impl Default for BehaviorAdmireItemPriority {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.admire_item`. Enables the mob to admire items that have been configured as admirable. Must be used in combination with the admire_item component.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorAdmireItem {
    ///The sound event to play when admiring the item.
    pub admire_item_sound: Option<String>,
    ///The event to run when admiring the item.
    pub on_admire_item_start: Option<crate::types::BedrockValue>,
    ///The event to run when no longer admiring the item.
    pub on_admire_item_stop: Option<crate::types::BedrockValue>,
    ///priority
    pub priority: Option<BehaviorAdmireItemPriority>,
    ///The range of time in seconds to randomly wait before playing the sound again.
    pub sound_interval: Option<crate::types::RangeOrVal<f32>>,
}
impl Default for BehaviorAdmireItem {
    fn default() -> Self {
        Self {
            admire_item_sound: None,
            on_admire_item_start: None,
            on_admire_item_stop: None,
            priority: None,
            sound_interval: Some(crate::types::RangeOrVal::Fixed(0f32)),
        }
    }
}
