use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:balloonable`. allows the entity to have a balloon attached and defines the conditions and events for the entity when is ballooned.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct Balloonable {
    ///Mass that the entity has when computing balloon pull forces.
    pub mass: Option<f32>,
    ///Distance in blocks where the balloon breaks.
    pub max_distance: Option<f32>,
    ///Event to call when the entity is ballooned.
    pub on_balloon: Option<String>,
    ///Event to call when the entity is unballooned.
    pub on_unballoon: Option<String>,
    ///Distance in blocks where the 'spring' effect lifts the entity.
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
