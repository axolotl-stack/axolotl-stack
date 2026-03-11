use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:player.exhaustion`. Defines the player's exhaustion level.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct PlayerExhaustion {
    ///The maximum player exhaustion of this entity.
    pub max: Option<i32>,
    ///The initial value of the player exhaustion.
    pub value: i32,
}
impl Default for PlayerExhaustion {
    fn default() -> Self {
        Self {
            max: None,
            value: 0i32,
        }
    }
}
