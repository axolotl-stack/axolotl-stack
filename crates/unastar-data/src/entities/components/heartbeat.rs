use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:heartbeat`. defines the entity's heartbeat..
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct Heartbeat {
    /// interval
    pub interval: Option<String>,
    /// sound_event
    pub sound_event: Option<String>,
}
impl Default for Heartbeat {
    fn default() -> Self {
        Self {
            interval: Some("1.00".to_string()),
            sound_event: Some("heartbeat".to_string()),
        }
    }
}
