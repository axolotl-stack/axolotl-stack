use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct EquipmentSlotDropChance {
    ///The chance that the item in this slot will drop.
    pub drop_chance: Option<f32>,
    ///The slot in which the item will drop from.
    pub slot: Option<String>,
}
impl Default for EquipmentSlotDropChance {
    fn default() -> Self {
        Self {
            drop_chance: None,
            slot: None,
        }
    }
}
/// Bedrock component `minecraft:equipment`. Sets the equipment table to use for the entity.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct Equipment {
    ///A list of slots with the chance to drop an equipped item from that slot.
    pub slot_drop_chance: Option<Vec<EquipmentSlotDropChance>>,
    ///The file path to the equipment table, relative to the behavior pack's root.
    pub table: Option<String>,
}
impl Default for Equipment {
    fn default() -> Self {
        Self {
            slot_drop_chance: None,
            table: None,
        }
    }
}
