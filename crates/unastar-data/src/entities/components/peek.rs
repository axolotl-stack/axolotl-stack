use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct PeekOnClose {
    ///The event to fire.
    pub event: Option<String>,
    ///filters
    pub filters: Option<crate::types::BedrockValue>,
    ///The target of the event.
    pub target: Option<String>,
}
impl Default for PeekOnClose {
    fn default() -> Self {
        Self {
            event: None,
            filters: None,
            target: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct PeekOnOpen {
    ///The event to fire.
    pub event: Option<String>,
    ///filters
    pub filters: Option<crate::types::BedrockValue>,
    ///The target of the event.
    pub target: Option<String>,
}
impl Default for PeekOnOpen {
    fn default() -> Self {
        Self {
            event: None,
            filters: None,
            target: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct PeekOnTargetOpen {
    ///The event to fire.
    pub event: Option<String>,
    ///filters
    pub filters: Option<crate::types::BedrockValue>,
    ///The target of the event.
    pub target: Option<String>,
}
impl Default for PeekOnTargetOpen {
    fn default() -> Self {
        Self {
            event: None,
            filters: None,
            target: None,
        }
    }
}
/// Bedrock component `minecraft:peek`. Defines the entity's `peek` behavior, defining the events that should be called during it.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct Peek {
    ///Event to call when the entity is done peeking.
    pub on_close: Option<PeekOnClose>,
    ///Event to call when the entity starts peeking.
    pub on_open: Option<PeekOnOpen>,
    ///Event to call when the entity's target entity starts peeking.
    pub on_target_open: Option<PeekOnTargetOpen>,
}
impl Default for Peek {
    fn default() -> Self {
        Self {
            on_close: None,
            on_open: None,
            on_target_open: None,
        }
    }
}
