use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.random_look_around_and_sit`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorRandomLookAroundAndSit {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
