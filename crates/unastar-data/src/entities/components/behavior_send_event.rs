use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.send_event`
#[derive(Component, Debug, Clone, Default, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorSendEvent {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
