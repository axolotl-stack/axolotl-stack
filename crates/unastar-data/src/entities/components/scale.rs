use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:scale`. Sets the entity's visual size.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct Scale {
    ///The value of the scale. 1.0 means the entity will appear at the scale they are defined in their model. Higher numbers make the entity bigger
    pub value: f32,
}
impl Default for Scale {
    fn default() -> Self {
        Self { value: 1f32 }
    }
}
