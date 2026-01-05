use bevy_ecs::prelude::*;
/// Component DTO for `minecraft:variable_max_auto_step`
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct VariableMaxAutoStep {
    /// Raw data - schema not yet defined
    pub data: Option<serde_json::Value>,
}
