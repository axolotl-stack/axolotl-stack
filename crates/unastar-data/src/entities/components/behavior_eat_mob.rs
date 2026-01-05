use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.eat_mob`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorEatMob {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
