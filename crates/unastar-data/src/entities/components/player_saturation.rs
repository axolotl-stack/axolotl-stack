use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:player.saturation`. Defines the player's need for food.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct PlayerSaturation {
    /// max
    pub max: Option<i32>,
    /// value
    pub value: i32,
}
impl Default for PlayerSaturation {
    fn default() -> Self {
        Self {
            max: None,
            value: 0,
        }
    }
}
