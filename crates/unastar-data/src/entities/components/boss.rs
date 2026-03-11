use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:boss`. The current state of the boss for updating the boss HUD.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct Boss {
    ///The Maximum distance from the boss at which the boss's health bar is present on the players screen.
    pub hud_range: Option<i32>,
    ///The name that will be displayed above the boss's health bar.
    pub name: Option<String>,
    ///Whether the sky should darken in the presence of the boss.
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
