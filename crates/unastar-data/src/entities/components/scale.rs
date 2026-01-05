use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:scale`
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct Scale {
    /// value
    pub value: f32,
}
impl Default for Scale {
    fn default() -> Self {
        Self { value: 1f32 }
    }
}
