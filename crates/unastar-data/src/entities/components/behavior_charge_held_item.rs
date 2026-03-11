use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorChargeHeldItemPriority {}
impl Default for BehaviorChargeHeldItemPriority {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.charge_held_item`. Allows an entity to charge and use their held item.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorChargeHeldItem {
    ///The list of items that can be used to charge the held item. This list is required and must have at least one item in it.
    pub items: Option<Vec<crate::types::BedrockValue>>,
    ///priority
    pub priority: Option<BehaviorChargeHeldItemPriority>,
}
impl Default for BehaviorChargeHeldItem {
    fn default() -> Self {
        Self {
            items: Some(vec![crate::types::BedrockValue::String("NA".to_string())]),
            priority: None,
        }
    }
}
