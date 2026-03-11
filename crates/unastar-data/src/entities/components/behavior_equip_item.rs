use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorEquipItemPriority {}
impl Default for BehaviorEquipItemPriority {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.equip_item`. The entity puts on the desired equipment.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct BehaviorEquipItem {
    ///priority
    pub priority: Option<BehaviorEquipItemPriority>,
}
impl Default for BehaviorEquipItem {
    fn default() -> Self {
        Self { priority: None }
    }
}
