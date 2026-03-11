use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:insomnia`. Adds a timer since last rested to see if phantoms should spawn.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct Insomnia {
    /// days_until_insomnia
    pub days_until_insomnia: Option<f32>,
}
impl Default for Insomnia {
    fn default() -> Self {
        Self {
            days_until_insomnia: Some(3f32),
        }
    }
}
