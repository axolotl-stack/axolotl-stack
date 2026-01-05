use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.stalk_and_pounce_on_target`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorStalkAndPounceOnTarget {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
