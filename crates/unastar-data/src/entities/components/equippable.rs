use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct EquippableSlotsOnEquip {
    ///The event to fire.
    pub event: Option<String>,
    ///filters
    pub filters: Option<crate::types::BedrockValue>,
    ///The target of the event.
    pub target: Option<String>,
}
impl Default for EquippableSlotsOnEquip {
    fn default() -> Self {
        Self {
            event: None,
            filters: None,
            target: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct EquippableSlotsOnUnequip {
    ///The event to fire.
    pub event: Option<String>,
    ///filters
    pub filters: Option<crate::types::BedrockValue>,
    ///The target of the event.
    pub target: Option<String>,
}
impl Default for EquippableSlotsOnUnequip {
    fn default() -> Self {
        Self {
            event: None,
            filters: None,
            target: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct EquippableSlots {
    ///The list of items that can go in this slot.
    pub accepted_items: Option<Vec<crate::types::BedrockValue>>,
    ///Text to be displayed when the entity can be equipped with this item when playing with Touch-screen controls.
    pub interact_text: Option<String>,
    ///Identifier of the item that can be equipped for this slot.
    pub item: Option<crate::types::BedrockValue>,
    ///Event to trigger when this entity is equipped with this item.
    pub on_equip: Option<EquippableSlotsOnEquip>,
    ///Event to trigger when this item is removed from this entity.
    pub on_unequip: Option<EquippableSlotsOnUnequip>,
    ///The slot number of this slot.
    pub slot: Option<i32>,
}
impl Default for EquippableSlots {
    fn default() -> Self {
        Self {
            accepted_items: None,
            interact_text: None,
            item: None,
            on_equip: None,
            on_unequip: None,
            slot: None,
        }
    }
}
/// Bedrock component `minecraft:equippable`. Defines an entity's behavior for having items equipped to it.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct Equippable {
    ///List of slots and the item that can be equipped.
    pub slots: Option<Vec<EquippableSlots>>,
}
impl Default for Equippable {
    fn default() -> Self {
        Self { slots: None }
    }
}
