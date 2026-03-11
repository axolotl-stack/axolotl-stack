use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:behavior.restrict_open_door`. Allows the mob to stay indoors during night time.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorRestrictOpenDoor {
    /// priority
    pub priority: Option<i32>,
}
impl Default for BehaviorRestrictOpenDoor {
    fn default() -> Self {
        Self { priority: None }
    }
}
