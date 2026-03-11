use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorOpenDoorPriority {}
impl Default for BehaviorOpenDoorPriority {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.open_door`. Allows the mob to open doors. Requires the mob to be able to path through doors, otherwise the mob won't even want to try opening them.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorOpenDoor {
    ///If true, the mob will close the door after opening it and going through it.
    pub close_door_after: Option<bool>,
    ///priority
    pub priority: Option<BehaviorOpenDoorPriority>,
}
impl Default for BehaviorOpenDoor {
    fn default() -> Self {
        Self {
            close_door_after: Some(true),
            priority: None,
        }
    }
}
