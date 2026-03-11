use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct EntitySensorSubsensors {
    ///How many seconds should elapse before the subsensor can once again sense for entities. The cooldown is applied on top of the base 1 tick (0.05 seconds) delay. Negative values will result in no cooldown being used.
    pub cooldown: Option<f32>,
    ///event.
    pub event: Option<String>,
    ///event_filters
    pub event_filters: Option<crate::types::BedrockValue>,
    ///The maximum number of entities that must pass the filter conditions for the event to send.
    pub maximum_count: Option<i32>,
    ///The minimum number of entities that must pass the filter conditions for the event to send.
    pub minimum_count: Option<i32>,
    ///The maximum distance another entity can be from this and have the filters checked against it.
    pub range: Option<Vec<f32>>,
    ///If true requires all nearby entities to pass the filter conditions for the event to send.
    pub require_all: Option<bool>,
    ///The maximum distance another entity can be from this and have the filters checked against it.
    pub sensor_range: Option<f32>,
    ///Vertical offset applied to the entity's position when computing the distance from other entities.
    pub y_offset: Option<f32>,
}
impl Default for EntitySensorSubsensors {
    fn default() -> Self {
        Self {
            cooldown: None,
            event: None,
            event_filters: None,
            maximum_count: None,
            minimum_count: None,
            range: None,
            require_all: None,
            sensor_range: None,
            y_offset: None,
        }
    }
}
/// Bedrock component `minecraft:entity_sensor`. A component that fires an event when a set of conditions are met by other entities within the defined range.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct EntitySensor {
    ///How many seconds should elapse before the subsensor can once again sense for entities. The cooldown is applied on top of the base 1 tick (0.05 seconds) delay. Negative values will result in no cooldown being used.
    pub cooldown: Option<f32>,
    ///event.
    pub event: Option<String>,
    ///event_filters
    pub event_filters: Option<crate::types::BedrockValue>,
    ///Limits the search to Players only for all subsensors.
    pub find_players_only: Option<bool>,
    ///The maximum number of entities that must pass the filter conditions for the event to send.
    pub maximum_count: Option<i32>,
    ///The minimum number of entities that must pass the filter conditions for the event to send.
    pub minimum_count: Option<i32>,
    ///The maximum distance another entity can be from this and have the filters checked against it.
    pub range: Option<Vec<f32>>,
    ///If true the sensor range is additive on top of the entity's size.
    pub relative_range: Option<bool>,
    ///If true requires all nearby entities to pass the filter conditions for the event to send.
    pub require_all: Option<bool>,
    ///The maximum distance another entity can be from this and have the filters checked against it.
    pub sensor_range: Option<f32>,
    ///The list of subsensors.
    pub subsensors: Option<Vec<EntitySensorSubsensors>>,
    ///Vertical offset applied to the entity's position when computing the distance from other entities.
    pub y_offset: Option<f32>,
}
impl Default for EntitySensor {
    fn default() -> Self {
        Self {
            cooldown: None,
            event: None,
            event_filters: None,
            find_players_only: Some(false),
            maximum_count: None,
            minimum_count: None,
            range: None,
            relative_range: Some(true),
            require_all: None,
            sensor_range: None,
            subsensors: None,
            y_offset: None,
        }
    }
}
