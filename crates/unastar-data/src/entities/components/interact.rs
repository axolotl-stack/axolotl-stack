use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct InteractInteractionsAddItems {
    ///File path, relative to the Behavior Pack's path, to the loot table file.
    pub table: Option<String>,
}
impl Default for InteractInteractionsAddItems {
    fn default() -> Self {
        Self { table: None }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct InteractInteractionsParticleOnStart {
    ///Whether or not the particle will appear closer to who performed the interaction.
    pub particle_offset_towards_interactor: Option<bool>,
    ///The type of particle that will be spawned.
    pub particle_type: Option<String>,
    ///Will offset the particle this amount in the y direction.
    pub particle_y_offset: Option<f32>,
}
impl Default for InteractInteractionsParticleOnStart {
    fn default() -> Self {
        Self {
            particle_offset_towards_interactor: None,
            particle_type: None,
            particle_y_offset: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct InteractInteractionsRepairEntityItem {
    ///How much of the item durability should be restored upon interaction.
    pub amount: Option<i32>,
    ///The entity's slot containing the item to be repaired.
    pub slot: Option<String>,
}
impl Default for InteractInteractionsRepairEntityItem {
    fn default() -> Self {
        Self {
            amount: None,
            slot: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct InteractInteractionsSpawnItems {
    ///File path, relative to the Behavior Pack's path, to the loot table file.
    pub table: Option<String>,
    ///Will offset the items spawn position this amount in the y direction.
    pub y_offset: Option<f32>,
}
impl Default for InteractInteractionsSpawnItems {
    fn default() -> Self {
        Self {
            table: None,
            y_offset: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct InteractInteractions {
    ///Loot table with items to add to the player's inventory upon successful interaction.
    pub add_items: Option<InteractInteractionsAddItems>,
    ///Allows entity to admire the item. Requires "minecraft:admire_item" and "minecraft:behavior.admire_item" to work.
    pub admire: Option<bool>,
    ///Allows entity to barter with the item. Requires "minecraft:barter" to work.
    pub barter: Option<bool>,
    ///Time in seconds before this entity can be interacted with again.
    pub cooldown: Option<f32>,
    ///Time in seconds before this entity can be interacted with after being attacked.
    pub cooldown_after_being_attacked: Option<f32>,
    ///The entity's equipment slot to remove and drop the item from, if any, upon successful interaction.
    pub drop_item_slot: Option<String>,
    ///Will offset the item drop position this amount in the y direction. Requires "drop_item_slot" to be specified.
    pub drop_item_y_offset: Option<f32>,
    ///The entity's equipment slot to equip the item to, if any, upon successful interaction.
    pub equip_item_slot: Option<String>,
    ///UNDOCUMENTED Item to give to the player upon successful interaction.
    pub give_item: Option<bool>,
    ///The amount of health this entity will recover or hurt when interacting with this item. Negative values will harm the entity.
    pub health_amount: Option<i32>,
    ///The amount of damage the item will take when used to interact with this entity. A value of 0 means the item won't lose durability.
    pub hurt_item: Option<i32>,
    ///Text to show when the player is able to interact in this way with this entity when playing with Touch-screen controls.
    pub interact_text: Option<String>,
    ///Event to fire when the interaction occurs.
    pub on_interact: Option<crate::types::BedrockValue>,
    ///Particle effect that will be triggered at the start of the interaction.
    pub particle_on_start: Option<InteractInteractionsParticleOnStart>,
    ///List of sounds to play when the interaction occurs.
    pub play_sounds: Option<String>,
    ///Allows to repair one of the entity's items.
    pub repair_entity_item: Option<InteractInteractionsRepairEntityItem>,
    ///List of entities to spawn when the interaction occurs.
    pub spawn_entities: Option<String>,
    ///Loot table with items to drop on the ground upon successful interaction.
    pub spawn_items: Option<InteractInteractionsSpawnItems>,
    ///If true, the player will do the "swing" animation when interacting with this entity.
    pub swing: Option<bool>,
    ///UNDOCUMENTED Takes an item from the player.
    pub take_item: Option<bool>,
    ///The feed item used will transform to this item upon successful interaction. Format: itemName:auxValue
    pub transform_to_item: Option<String>,
    ///If true, the interaction will use an item.
    pub use_item: Option<bool>,
    ///Vibration to emit when the interaction occurs. Admitted values are none (no vibration emitted), shear, entity_die, entity_act, entity_interact.
    pub vibration: Option<String>,
}
impl Default for InteractInteractions {
    fn default() -> Self {
        Self {
            add_items: None,
            admire: None,
            barter: None,
            cooldown: None,
            cooldown_after_being_attacked: None,
            drop_item_slot: None,
            drop_item_y_offset: None,
            equip_item_slot: None,
            give_item: None,
            health_amount: None,
            hurt_item: None,
            interact_text: None,
            on_interact: None,
            particle_on_start: None,
            play_sounds: None,
            repair_entity_item: None,
            spawn_entities: None,
            spawn_items: None,
            swing: None,
            take_item: None,
            transform_to_item: None,
            use_item: None,
            vibration: None,
        }
    }
}
/// Bedrock component `minecraft:interact`. Defines interactions with this entity.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct Interact {
    ///The interactions.
    pub interactions: Option<Vec<InteractInteractions>>,
}
impl Default for Interact {
    fn default() -> Self {
        Self { interactions: None }
    }
}
