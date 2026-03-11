use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:boss`. The current state of the boss for updating the boss HUD.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct Boss {
    /// hud_range
    pub hud_range: Option<i32>,
    /// name
    pub name: Option<String>,
    /// should_darken_sky
    pub should_darken_sky: Option<bool>,
}
impl Default for Boss {
    fn default() -> Self {
        Self {
            hud_range: Some(55i32),
            name: Some("55".to_string()),
            should_darken_sky: Some(false),
        }
    }
}
