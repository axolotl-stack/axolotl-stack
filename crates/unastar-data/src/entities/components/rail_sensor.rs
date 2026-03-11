use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct RailSensorOnActivate {
    ///The event to fire.
    pub event: Option<String>,
    ///filters
    pub filters: Option<crate::types::BedrockValue>,
    ///The target of the event.
    pub target: Option<String>,
}
impl Default for RailSensorOnActivate {
    fn default() -> Self {
        Self {
            event: None,
            filters: None,
            target: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct RailSensorOnDeactivate {
    ///The event to fire.
    pub event: Option<String>,
    ///filters
    pub filters: Option<crate::types::BedrockValue>,
    ///The target of the event.
    pub target: Option<String>,
}
impl Default for RailSensorOnDeactivate {
    fn default() -> Self {
        Self {
            event: None,
            filters: None,
            target: None,
        }
    }
}
/// Bedrock component `minecraft:rail_sensor`. Defines the behavior of the entity when the rail gets activated or deactivated.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct RailSensor {
    ///If true, on tick this entity will trigger its on_deactivate behavior.
    pub check_block_types: Option<bool>,
    ///If true, this entity will eject all of its riders when it passes over an activated rail.
    pub eject_on_activate: Option<bool>,
    ///If true, this entity will eject all of its riders when it passes over a deactivated rail.
    pub eject_on_deactivate: Option<bool>,
    ///Event to call when the rail is activated.
    pub on_activate: Option<RailSensorOnActivate>,
    ///Event to call when the rail is deactivated.
    pub on_deactivate: Option<RailSensorOnDeactivate>,
    ///If true, command blocks will start ticking when passing over an activated rail.
    pub tick_command_block_on_activate: Option<bool>,
    ///If false, command blocks will stop ticking when passing over a deactivated rail.
    pub tick_command_block_on_deactivate: Option<bool>,
}
impl Default for RailSensor {
    fn default() -> Self {
        Self {
            check_block_types: Some(false),
            eject_on_activate: Some(true),
            eject_on_deactivate: Some(false),
            on_activate: None,
            on_deactivate: None,
            tick_command_block_on_activate: Some(true),
            tick_command_block_on_deactivate: Some(false),
        }
    }
}
