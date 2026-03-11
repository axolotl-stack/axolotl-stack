use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct LeashableOnLeash {
    ///The event to fire.
    pub event: Option<String>,
    ///filters
    pub filters: Option<crate::types::BedrockValue>,
    ///The target of the event.
    pub target: Option<String>,
}
impl Default for LeashableOnLeash {
    fn default() -> Self {
        Self {
            event: None,
            filters: None,
            target: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct LeashableOnUnleash {
    ///The event to fire.
    pub event: Option<String>,
    ///filters
    pub filters: Option<crate::types::BedrockValue>,
    ///The target of the event.
    pub target: Option<String>,
}
impl Default for LeashableOnUnleash {
    fn default() -> Self {
        Self {
            event: None,
            filters: None,
            target: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct LeashablePresets {
    ///filter
    pub filter: Option<crate::types::BedrockValue>,
    ///hard_distance
    pub hard_distance: Option<f32>,
    ///max_distance
    pub max_distance: Option<f32>,
    ///Adjusts the rotation at which the entity reaches equilibrium, when "spring_type" is set to "dampened" or "quad_dampened"
    pub rotation_adjustment: Option<f32>,
    ///soft_distance
    pub soft_distance: Option<f32>,
    /**Defines the type of spring-like force that pulls the entity towards its leash holder:
    - "bouncy": Simulates a highly elastic spring that never reaches an equilibrium if the leashed entity is suspended mid-air.
    - "dampened": Simulates a dampened spring attached to the front of the leashed entity's collision. It reaches an equilibrium if the entity is suspended mid-air and aligns with the movement direction.
    - "quad_dampened": Simulates four dampened springs connected to the center of each side of the entities' collisions. It reaches an equilibrium if the entity is suspended mid-air and gradually aligns with the leash holder over time.*/
    pub spring_type: Option<String>,
}
impl Default for LeashablePresets {
    fn default() -> Self {
        Self {
            filter: None,
            hard_distance: None,
            max_distance: None,
            rotation_adjustment: None,
            soft_distance: None,
            spring_type: None,
        }
    }
}
/// Bedrock component `minecraft:leashable`. Allows this entity to be leashed and defines the conditions and events for this entity when is leashed.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct Leashable {
    ///If true, players can cut both incoming and outgoing leashes by using shears on the entity.
    pub can_be_cut: Option<bool>,
    ///If true, players can leash this entity even if it is already leashed to another mob.
    pub can_be_stolen: Option<bool>,
    ///hard_distance
    pub hard_distance: Option<f32>,
    ///max_distance
    pub max_distance: Option<f32>,
    ///Event to call when this entity is leashed.
    pub on_leash: Option<LeashableOnLeash>,
    ///Event to call when this entity is unleashed.
    pub on_unleash: Option<LeashableOnUnleash>,
    ///When set to true, "on_unleash" does not trigger when the entity gets unleashed for other reasons such as being stolen or the leash breaking.
    pub on_unleash_interact_only: Option<bool>,
    ///Defines how this entity behaves when leashed to another entity. A preset is selected upon leashing and remains until the entity is leashed to something else. The first preset whose "filter" conditions are met will be applied; if none match, a default configuration is used instead.
    pub presets: Option<Vec<LeashablePresets>>,
    ///soft_distance
    pub soft_distance: Option<f32>,
}
impl Default for Leashable {
    fn default() -> Self {
        Self {
            can_be_cut: Some(true),
            can_be_stolen: Some(false),
            hard_distance: Some(6f32),
            max_distance: Some(0f32),
            on_leash: None,
            on_unleash: None,
            on_unleash_interact_only: Some(false),
            presets: None,
            soft_distance: Some(4f32),
        }
    }
}
