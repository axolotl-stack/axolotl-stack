use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:behavior.open_door`. Allows the mob to open doors. Requires the mob to be able to path through doors, otherwise the mob won't even want to try opening them.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorOpenDoor {
    /// close_door_after
    pub close_door_after: Option<bool>,
    /// priority
    pub priority: Option<i32>,
}
impl Default for BehaviorOpenDoor {
    fn default() -> Self {
        Self {
            close_door_after: Some(true),
            priority: None,
        }
    }
}
