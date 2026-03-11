use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct AmbientSoundIntervalEventNames {
    ///The condition that must be satisfied to select the given ambient sound.
    pub condition: Option<String>,
    ///Level sound event to be played as the ambient sound.
    pub event_name: Option<String>,
}
impl Default for AmbientSoundIntervalEventNames {
    fn default() -> Self {
        Self {
            condition: None,
            event_name: None,
        }
    }
}
/// Bedrock component `minecraft:ambient_sound_interval`. Sets the entity's delay between playing its ambient sound.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct AmbientSoundInterval {
    ///Level sound event to be played as the ambient sound.
    pub event_name: Option<String>,
    ///List of dynamic level sound events, with conditions for choosing between them. Evaluated in order, first one wins. If none evaluate to true, 'event_name' will take precedence.
    pub event_names: Option<Vec<AmbientSoundIntervalEventNames>>,
    ///Maximum time in seconds to randomly add to the ambient sound delay time.
    pub range: Option<f32>,
    ///Minimum time in seconds before the entity plays its ambient sound again.
    pub value: f32,
}
impl Default for AmbientSoundInterval {
    fn default() -> Self {
        Self {
            event_name: Some("ambient".to_string()),
            event_names: None,
            range: Some(16f32),
            value: 8f32,
        }
    }
}
