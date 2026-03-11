use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:reflect_projectiles`. [EXPERIMENTAL] Allows an entity to reflect projectiles.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct ReflectProjectiles {
    ///A Molang expression defining the angle in degrees to add to the projectile's y axis rotation.
    pub azimuth_angle: Option<crate::types::MolangOr<f32>>,
    ///A Molang expression defining the angle in degrees to add to the projectile's x axis rotation.
    pub elevation_angle: Option<crate::types::MolangOr<f32>>,
    ///An array of strings defining the types of projectiles that are reflected when they hit the entity.
    pub reflected_projectiles: Option<Vec<String>>,
    ///A Molang expression defining the velocity scaling of the reflected projectile. Values below 1 decrease the projectile's velocity, and values above 1 increase it.
    pub reflection_scale: Option<crate::types::MolangOr<f32>>,
    ///A string defining the name of the sound event to be played when a projectile is reflected. "reflect" unless specified.
    pub reflection_sound: Option<String>,
}
impl Default for ReflectProjectiles {
    fn default() -> Self {
        Self {
            azimuth_angle: Some(crate::types::MolangOr::Expr("0".to_string())),
            elevation_angle: Some(crate::types::MolangOr::Expr("0".to_string())),
            reflected_projectiles: None,
            reflection_scale: Some(crate::types::MolangOr::Expr("1".to_string())),
            reflection_sound: Some("reflect".to_string()),
        }
    }
}
