use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.stomp_turtle_egg`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorStompTurtleEgg {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
