use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:equip_item`. The entity puts on the desired equipment.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct EquipItem {
    ///Specifies if equipped armor should be added to the armor slot or hand slot
    pub can_wear_armor: Option<bool>,
    ///List of items that the entity should not equip.
    pub excluded_items: Option<Vec<crate::types::BedrockValue>>,
}
impl Default for EquipItem {
    fn default() -> Self {
        Self {
            can_wear_armor: None,
            excluded_items: None,
        }
    }
}
