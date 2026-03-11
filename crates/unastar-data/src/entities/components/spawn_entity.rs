use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct SpawnEntityEntitiesSpawnItemEvent {
    ///The event to fire.
    pub event: Option<String>,
    ///filters
    pub filters: Option<crate::types::BedrockValue>,
    ///The target of the event.
    pub target: Option<String>,
}
impl Default for SpawnEntityEntitiesSpawnItemEvent {
    fn default() -> Self {
        Self {
            event: None,
            filters: None,
            target: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct SpawnEntityEntities {
    ///If present, the specified entity will only spawn if the filter evaluates to true.
    pub filters: Option<crate::types::BedrockValue>,
    ///Maximum amount of time to randomly wait in seconds before another entity is spawned.
    pub max_wait_time: Option<i32>,
    ///Minimum amount of time to randomly wait in seconds before another entity is spawned.
    pub min_wait_time: Option<i32>,
    ///The number of entities of this type to spawn each time that this triggers.
    pub num_to_spawn: Option<i32>,
    ///If true, this the spawned entity will be leashed to the parent.
    pub should_leash: Option<bool>,
    ///If true, this component will only ever spawn the specified entity once.
    pub single_use: Option<bool>,
    ///Identifier of the entity to spawn, leave empty to spawn the item defined above instead.
    pub spawn_entity: Option<String>,
    ///Event to call when the entity is spawned.
    pub spawn_event: Option<String>,
    ///Item identifier of the item to spawn.
    pub spawn_item: Option<crate::types::BedrockValue>,
    ///Event to call on this entity when the item is spawned.
    pub spawn_item_event: Option<SpawnEntityEntitiesSpawnItemEvent>,
    ///Method to use to spawn the entity.
    pub spawn_method: Option<String>,
    ///Identifier of the sound effect to play when the entity is spawned.
    pub spawn_sound: Option<String>,
}
impl Default for SpawnEntityEntities {
    fn default() -> Self {
        Self {
            filters: None,
            max_wait_time: None,
            min_wait_time: None,
            num_to_spawn: None,
            should_leash: None,
            single_use: None,
            spawn_entity: None,
            spawn_event: None,
            spawn_item: None,
            spawn_item_event: None,
            spawn_method: None,
            spawn_sound: None,
        }
    }
}
/// Bedrock component `minecraft:spawn_entity`. Adds a timer after which this entity will spawn another entity or item (similar to vanilla's chicken's egg-laying behavior).
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct SpawnEntity {
    ///The entities to spawn.
    pub entities: Option<Vec<SpawnEntityEntities>>,
}
impl Default for SpawnEntity {
    fn default() -> Self {
        Self { entities: None }
    }
}
