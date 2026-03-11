use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:leashable_to`. Allows players to leash entities to this entity, retrieve those already leashed to it, or free them using shears.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct LeashableTo {
    /// can_retrieve_from
    pub can_retrieve_from: Option<bool>,
}
impl Default for LeashableTo {
    fn default() -> Self {
        Self {
            can_retrieve_from: Some(false),
        }
    }
}
