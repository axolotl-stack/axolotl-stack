use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorRestrictOpenDoorPriority {}
impl Default for BehaviorRestrictOpenDoorPriority {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.restrict_open_door`. Allows the mob to stay indoors during night time.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorRestrictOpenDoor {
    ///priority
    pub priority: Option<BehaviorRestrictOpenDoorPriority>,
}
impl Default for BehaviorRestrictOpenDoor {
    fn default() -> Self {
        Self { priority: None }
    }
}
