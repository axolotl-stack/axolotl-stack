use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:player.level`. Defines the player's level.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct PlayerLevel {
    ///The maximum player level value of the entity.
    pub max: Option<i32>,
    ///The initial value of the player level.
    pub value: i32,
}
impl Default for PlayerLevel {
    fn default() -> Self {
        Self {
            max: None,
            value: 0i32,
        }
    }
}
