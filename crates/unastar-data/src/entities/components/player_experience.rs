use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:player.experience`. Defines how much experience each player action should take.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct PlayerExperience {
    ///The maximum player experience of this entity.
    pub max: Option<i32>,
    ///The initial value of the player experience.
    pub value: i32,
}
impl Default for PlayerExperience {
    fn default() -> Self {
        Self {
            max: Some(5i32),
            value: 1i32,
        }
    }
}
