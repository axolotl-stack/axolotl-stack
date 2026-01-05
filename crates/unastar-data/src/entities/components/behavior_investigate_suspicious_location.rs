use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.investigate_suspicious_location`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorInvestigateSuspiciousLocation {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
