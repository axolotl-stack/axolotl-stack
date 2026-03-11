use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:behavior.equip_item`. The entity puts on the desired equipment.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct BehaviorEquipItem {
    /// priority
    pub priority: Option<i32>,
}
impl Default for BehaviorEquipItem {
    fn default() -> Self {
        Self { priority: None }
    }
}
