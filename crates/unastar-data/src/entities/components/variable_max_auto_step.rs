use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:variable_max_auto_step`. Entities with this component will have a maximum auto step height that is different depending on wether they are on a block that prevents jumping. Incompatible with "runtime_identifier": "minecraft:horse".
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct VariableMaxAutoStep {
    /// base_value
    pub base_value: Option<f32>,
    /// controlled_value
    pub controlled_value: Option<f32>,
    /// jump_prevented_value
    pub jump_prevented_value: Option<f32>,
}
impl Default for VariableMaxAutoStep {
    fn default() -> Self {
        Self {
            base_value: Some(0.5625f32),
            controlled_value: Some(0.5625f32),
            jump_prevented_value: Some(0.5625f32),
        }
    }
}
