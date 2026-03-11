use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:behavior.stay_near_noteblock`
#[derive(Component, Debug, Clone, Default, PartialEq)]
#[component(storage = "SparseSet")]
pub struct BehaviorStayNearNoteblock {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
