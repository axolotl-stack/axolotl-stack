use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:behavior.eat_carried_item`. If the mob is carrying a food item, the mob will eat it and the effects will be applied to the mob.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorEatCarriedItem {
    /// delay_before_eating
    pub delay_before_eating: Option<f32>,
    /// priority
    pub priority: Option<i32>,
}
impl Default for BehaviorEatCarriedItem {
    fn default() -> Self {
        Self {
            delay_before_eating: None,
            priority: None,
        }
    }
}
