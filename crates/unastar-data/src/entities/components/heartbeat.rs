use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:heartbeat`. defines the entity's heartbeat..
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct Heartbeat {
    ///A Molang expression defining the inter-beat interval in seconds. A value of zero or less means no heartbeat..
    pub interval: Option<crate::types::MolangOr<f32>>,
    ///Level sound event to be played as the heartbeat sound.
    pub sound_event: Option<String>,
}
impl Default for Heartbeat {
    fn default() -> Self {
        Self {
            interval: Some(crate::types::MolangOr::Expr("1.00".to_string())),
            sound_event: Some("heartbeat".to_string()),
        }
    }
}
