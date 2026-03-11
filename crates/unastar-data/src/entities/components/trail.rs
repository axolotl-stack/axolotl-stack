use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:trail`. Defines the entity's trail to carry items.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct Trail {
    ///The type of block you wish to be spawned by the entity as it move about the world. Solid blocks may not be spawned at an offset of ().
    pub block_type: Option<String>,
    ///One or more conditions that must be met in order to cause the chosen block type to spawn.
    pub spawn_filter: Option<crate::types::BedrockValue>,
    ///The distance from the entities current position to spawn the block. Capped at up to 16 blocks away. The X value is left/right(-/+), the Z value is backward/forward(-/+), the Y value is below/above(-/+).
    pub spawn_offset: Option<Vec<f32>>,
}
impl Default for Trail {
    fn default() -> Self {
        Self {
            block_type: Some("air".to_string()),
            spawn_filter: None,
            spawn_offset: Some(vec![0f32, 0f32, 0f32]),
        }
    }
}
