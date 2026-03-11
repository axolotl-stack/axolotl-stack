use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:player.level`. Defines the player's level.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct PlayerLevel {
    /// max
    pub max: Option<i32>,
    /// value
    pub value: i32,
}
impl Default for PlayerLevel {
    fn default() -> Self {
        Self {
            max: None,
            value: 0,
        }
    }
}
