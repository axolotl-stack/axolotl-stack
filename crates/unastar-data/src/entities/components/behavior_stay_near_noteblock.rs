use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorStayNearNoteblockControlFlags {}
impl Default for BehaviorStayNearNoteblockControlFlags {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorStayNearNoteblockPriority {}
impl Default for BehaviorStayNearNoteblockPriority {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.stay_near_noteblock`. [EXPERIMENTAL BEHAVIOR] The entity will attempt to toss the items from its inventory to a nearby recently played noteblock.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorStayNearNoteblock {
    ///control_flags
    pub control_flags: Option<BehaviorStayNearNoteblockControlFlags>,
    ///Sets the time an entity should stay near a noteblock after hearing it.
    pub listen_time: Option<i32>,
    ///priority
    pub priority: Option<BehaviorStayNearNoteblockPriority>,
    ///Sets the entity's speed when moving toward the block.
    pub speed: Option<f32>,
    ///Sets the distance the entity needs to be away from the block to attempt to start the goal.
    pub start_distance: Option<f32>,
    ///Sets the distance from the block the entity will attempt to reach.
    pub stop_distance: Option<f32>,
}
impl Default for BehaviorStayNearNoteblock {
    fn default() -> Self {
        Self {
            control_flags: Some(BehaviorStayNearNoteblockControlFlags {}),
            listen_time: Some(30i32),
            priority: Some(BehaviorStayNearNoteblockPriority {}),
            speed: Some(1f32),
            start_distance: Some(10f32),
            stop_distance: Some(2f32),
        }
    }
}
