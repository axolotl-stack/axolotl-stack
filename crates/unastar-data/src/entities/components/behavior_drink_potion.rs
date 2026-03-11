use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorDrinkPotionPotions {
    ///The percent chance (from 0.0 to 1.0) of this potion being selected when searching for a potion to use.
    pub chance: f32,
    ///The filters to use when determining if this potion can be selected.
    pub filters: crate::types::BedrockValue,
    ///The registry ID of the potion to use.
    pub id: i32,
}
impl Default for BehaviorDrinkPotionPotions {
    fn default() -> Self {
        Self {
            chance: 0f32,
            filters: crate::types::BedrockValue::Null,
            id: 0i32,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorDrinkPotionPriority {}
impl Default for BehaviorDrinkPotionPriority {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorDrinkPotionSpeedMultiplier {}
impl Default for BehaviorDrinkPotionSpeedMultiplier {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.drink_potion`. Allows the mob to drink potions based on specified environment conditions.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorDrinkPotion {
    ///A list of potions that this entity can drink.
    pub potions: Option<Vec<BehaviorDrinkPotionPotions>>,
    ///priority
    pub priority: Option<BehaviorDrinkPotionPriority>,
    ///Movement speed modifier of the mob when using this AI Goal.
    pub speed_modifier: Option<crate::types::BedrockValue>,
    ///speed_multiplier
    pub speed_multiplier: Option<BehaviorDrinkPotionSpeedMultiplier>,
}
impl Default for BehaviorDrinkPotion {
    fn default() -> Self {
        Self {
            potions: None,
            priority: None,
            speed_modifier: Some(crate::types::BedrockValue::Integer(0i64)),
            speed_multiplier: None,
        }
    }
}
