use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorEatCarriedItemPriority {}
impl Default for BehaviorEatCarriedItemPriority {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.eat_carried_item`. If the mob is carrying a food item, the mob will eat it and the effects will be applied to the mob.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorEatCarriedItem {
    ///Time in seconds the mob should wait before eating the item.
    pub delay_before_eating: Option<f32>,
    ///priority
    pub priority: Option<BehaviorEatCarriedItemPriority>,
}
impl Default for BehaviorEatCarriedItem {
    fn default() -> Self {
        Self {
            delay_before_eating: None,
            priority: None,
        }
    }
}
