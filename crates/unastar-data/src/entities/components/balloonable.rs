use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:balloonable`. allows the entity to have a balloon attached and defines the conditions and events for the entity when is ballooned.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct Balloonable {
    /// mass
    pub mass: Option<f32>,
    /// max_distance
    pub max_distance: Option<f32>,
    /// on_balloon
    pub on_balloon: Option<String>,
    /// on_unballoon
    pub on_unballoon: Option<String>,
    /// soft_distance
    pub soft_distance: Option<f32>,
}
impl Default for Balloonable {
    fn default() -> Self {
        Self {
            mass: None,
            max_distance: None,
            on_balloon: None,
            on_unballoon: None,
            soft_distance: None,
        }
    }
}
