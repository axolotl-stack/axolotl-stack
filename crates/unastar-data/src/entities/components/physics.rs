use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:physics`. Defines the physical properties of an actor, including whether it is affected by gravity, whether it collides with objects, or whether it is pushed to the closest space.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct Physics {
    ///Whether or not the entity collides with things.
    pub has_collision: Option<bool>,
    ///Whether or not the entity is affected by gravity.
    pub has_gravity: Option<bool>,
    ///Whether or not the entity is pushed to the closest space.
    pub push_towards_closest_space: Option<bool>,
}
impl Default for Physics {
    fn default() -> Self {
        Self {
            has_collision: Some(true),
            has_gravity: Some(true),
            push_towards_closest_space: Some(false),
        }
    }
}
