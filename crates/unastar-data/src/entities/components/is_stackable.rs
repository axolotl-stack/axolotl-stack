use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:is_stackable`. Sets that this entity can be stacked.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct IsStackable {
    ///UNDOCUMENTED.
    pub value: bool,
}
impl Default for IsStackable {
    fn default() -> Self {
        Self { value: false }
    }
}
