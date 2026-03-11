use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:flying_speed`. Speed in Blocks that this entity flies at.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct FlyingSpeed {
    ///Flying speed in blocks per tick.
    pub value: f32,
}
impl Default for FlyingSpeed {
    fn default() -> Self {
        Self { value: 0.02f32 }
    }
}
