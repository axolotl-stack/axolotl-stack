use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.stay_while_sitting`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorStayWhileSitting {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
