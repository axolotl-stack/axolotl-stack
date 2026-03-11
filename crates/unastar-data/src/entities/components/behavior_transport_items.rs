use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorTransportItemsPriority {}
impl Default for BehaviorTransportItemsPriority {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.transport_items`. Allows a mob to transport items from and to containers
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorTransportItems {
    ///Whether the entity is allowed to simultaneously interact with a container that another non-player entity is already interacting with.
    pub allow_simultaneous_interaction: Option<bool>,
    ///A list of item descriptors that are the only items the mob is allowed to transport. If this and "disallowed_items" are both empty, then all items are allowed. If non-empty "disallowed_items" must be empty. Default value: empty.
    pub allowed_items: Option<Vec<String>>,
    ///A list of block descriptors that should be a container type to put items in. Default is any container
    pub destination_container_types: Option<Vec<crate::types::BedrockValue>>,
    ///A list of item descriptors that are the mob is not allowed to transport. If non-empty "allowed_items" must be empty. Default value: emtpy.
    pub disallowed_items: Option<Vec<String>>,
    ///When the mob cannot find a valid container to interact with, the goal will be disabled for this amount of time in seconds.
    pub idle_cooldown: Option<i32>,
    ///ime, in seconds, the mob will wait after spawning or after its available goals have changed (e.g. due to a component group update).
    pub initial_cooldown: Option<i32>,
    ///The amount of time spent interacting with the containers in seconds.
    pub interaction_time: Option<f32>,
    ///The maximum stack size that the mob will try to take from a container.
    pub max_stack_size: Option<i32>,
    ///The maximum number of containers the mob will visit before resetting. 0 is unlimited.
    pub max_visited_containers: Option<i32>,
    /**The strategy to use for placing the transported item.
    Any - always place if there is room,
    With matching - place if there is a matching item in the container,
    With matching or empty - like With matching but will also place in empty containers.*/
    pub place_strategy: Option<String>,
    ///priority
    pub priority: Option<BehaviorTransportItemsPriority>,
    ///The maximum horizontal and vertical distance at which to find containers for taking or placing items.
    pub search_distance: Option<crate::types::RangeOrVal<f32>>,
    ///Whether to select the nearest valid container or a random valid container in range.
    pub search_strategy: Option<String>,
    ///A list of block descriptors that should be a container type to get items from. Default is any container
    pub source_container_types: Option<Vec<crate::types::BedrockValue>>,
}
impl Default for BehaviorTransportItems {
    fn default() -> Self {
        Self {
            allow_simultaneous_interaction: Some(false),
            allowed_items: Some(vec![]),
            destination_container_types: Some(vec![]),
            disallowed_items: Some(vec![]),
            idle_cooldown: Some(20i32),
            initial_cooldown: Some(0i32),
            interaction_time: Some(3f32),
            max_stack_size: Some(64i32),
            max_visited_containers: Some(16i32),
            place_strategy: Some("any".to_string()),
            priority: Some(BehaviorTransportItemsPriority {}),
            search_distance: Some(crate::types::RangeOrVal::Range {
                min: 64f32,
                max: 32f32,
            }),
            search_strategy: Some("random".to_string()),
            source_container_types: Some(vec![]),
        }
    }
}
