use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct HealableItemsEffects {
    ///The amplifier of the effect.
    pub amplifier: Option<i32>,
    ///The duration of the effect.
    pub duration: Option<crate::types::MolangOr<i32>>,
    ///Effect name.
    pub name: Option<String>,
}
impl Default for HealableItemsEffects {
    fn default() -> Self {
        Self {
            amplifier: None,
            duration: None,
            name: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct HealableItems {
    ///effects
    pub effects: Option<Vec<HealableItemsEffects>>,
    ///The filter group that defines the conditions for using this item to heal the entity.
    pub filters: Option<crate::types::BedrockValue>,
    ///The amount of health this entity gains when fed this item.
    pub heal_amount: Option<i32>,
    ///Item that can be used to heal this entity.
    pub item: Option<crate::types::BedrockValue>,
    ///The item used will transform to this item upon successful interaction. Format: itemName:auxValue
    pub result_item: Option<crate::types::BedrockValue>,
}
impl Default for HealableItems {
    fn default() -> Self {
        Self {
            effects: None,
            filters: None,
            heal_amount: None,
            item: None,
            result_item: None,
        }
    }
}
/// Bedrock component `minecraft:healable`. Defines the interactions with this entity for healing it.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct Healable {
    ///filters
    pub filters: Option<crate::types::BedrockValue>,
    ///Determines if item can be used regardless of entity being at full health.
    pub force_use: Option<bool>,
    ///The array of items that can be used to heal this entity.
    pub items: Option<Vec<HealableItems>>,
}
impl Default for Healable {
    fn default() -> Self {
        Self {
            filters: None,
            force_use: Some(false),
            items: None,
        }
    }
}
