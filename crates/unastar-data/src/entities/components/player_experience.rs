use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:player.experience`. Defines how much experience each player action should take.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct PlayerExperience {
    /// max
    pub max: Option<i32>,
    /// value
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
