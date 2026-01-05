use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.mount_pathing`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct BehaviorMountPathing {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
