use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.run_around_like_crazy`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorRunAroundLikeCrazy {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
