use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:teleport`. Defines an entity's teleporting behavior.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct Teleport {
    ///Modifies the chance that the entity will teleport if the entity is in darkness.
    pub dark_teleport_chance: Option<f32>,
    ///Modifies the chance that the entity will teleport if the entity is in daylight.
    pub light_teleport_chance: Option<f32>,
    ///Maximum amount of time in seconds between random teleports.
    pub max_random_teleport_time: Option<f32>,
    ///Minimum amount of time in seconds between random teleports.
    pub min_random_teleport_time: Option<f32>,
    ///Entity will teleport to a random position within the area defined by this cube.
    pub random_teleport_cube: Option<Vec<f32>>,
    ///If true, the entity will teleport randomly.
    pub random_teleports: Option<bool>,
    ///Maximum distance the entity will teleport when chasing a target.
    pub target_distance: Option<f32>,
    ///The chance that the entity will teleport between 0.0 and 1.0. 1.0 means 100%
    pub target_teleport_chance: Option<f32>,
}
impl Default for Teleport {
    fn default() -> Self {
        Self {
            dark_teleport_chance: Some(0.01f32),
            light_teleport_chance: Some(0.01f32),
            max_random_teleport_time: Some(20f32),
            min_random_teleport_time: Some(0f32),
            random_teleport_cube: Some(vec![32f32, 16f32, 32f32]),
            random_teleports: Some(true),
            target_distance: Some(16f32),
            target_teleport_chance: Some(1f32),
        }
    }
}
