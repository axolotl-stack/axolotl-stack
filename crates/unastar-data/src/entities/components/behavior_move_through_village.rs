use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.move_through_village`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorMoveThroughVillage {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
