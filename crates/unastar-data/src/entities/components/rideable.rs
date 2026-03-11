use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct RideableSeats {
    ///Adds springiness to the camera movement when the camera moves back to its radius after being pushed closer to the player by and obstacle. A higher value means a stiffer spring. Value 0.0 is ignored.
    pub camera_relax_distance_smoothing: Option<f32>,
    ///Angle in degrees that a rider is allowed to rotate while riding this entity. Omit this property for no limit
    pub lock_rider_rotation: Option<f32>,
    ///Defines the maximum number of riders that can be riding this entity for this seat to be valid.
    pub max_rider_count: Option<i32>,
    ///Defines the minimum number of riders that need to be riding this entity before this seat can be used.
    pub min_rider_count: Option<i32>,
    ///Position of this seat relative to this entity's position.
    pub position: Option<Vec<f32>>,
    ///Offset to rotate riders by.
    pub rotate_rider_by: Option<crate::types::MolangOr<f32>>,
    ///Can be used to set a different camera radius when in third person or third person front camera. Value 0.0 is ignored.
    pub third_person_camera_radius: Option<f32>,
}
impl Default for RideableSeats {
    fn default() -> Self {
        Self {
            camera_relax_distance_smoothing: None,
            lock_rider_rotation: None,
            max_rider_count: None,
            min_rider_count: None,
            position: None,
            rotate_rider_by: None,
            third_person_camera_radius: None,
        }
    }
}
/// Bedrock component `minecraft:rideable`. Determines whether this entity can be ridden. Allows specifying the different seat positions and amount.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct Rideable {
    ///The seat that designates the driver of the entity. Entities with the "minecraft:behavior.controlled_by_player" goal ignore this field and give control to any player in any seat.
    pub controlling_seat: Option<i32>,
    ///If true, this entity can't be interacted with if the entity interacting with it is crouching.
    pub crouching_skip_interact: Option<bool>,
    /**Defines where riders are placed when dismounting this entity:
    - "default", riders are placed on a valid ground position around the entity, or at the center of the entity's collision box if none is found.
    - "on_top_center", riders are placed at the center of the top of the entity's collision box.*/
    pub dismount_mode: Option<String>,
    ///List of entities that can ride this entity.
    pub family_types: Option<Vec<String>>,
    ///The text to display when the player can interact with the entity when playing with Touch-screen controls.
    pub interact_text: Option<String>,
    ///Event to execute on the owner entity when an entity starts riding it.
    pub on_rider_enter_event: Option<crate::types::BedrockValue>,
    ///Event to execute on the owner entity when an entity stops riding it.
    pub on_rider_exit_event: Option<crate::types::BedrockValue>,
    ///The max width a mob can have to be a rider. A value of 0 ignores this parameter.
    pub passenger_max_width: Option<f32>,
    ///If true, this entity will pull in entities that are in the correct family_types into any available seats.
    pub pull_in_entities: Option<bool>,
    ///If true, this entity will be picked when looked at by the rider.
    pub rider_can_interact: Option<bool>,
    ///The number of entities that can ride this entity at the same time.
    pub seat_count: Option<i32>,
    ///The list of positions and number of riders for each position for entities riding this entity.
    pub seats: Option<Vec<RideableSeats>>,
}
impl Default for Rideable {
    fn default() -> Self {
        Self {
            controlling_seat: Some(0i32),
            crouching_skip_interact: Some(true),
            dismount_mode: Some("default".to_string()),
            family_types: None,
            interact_text: None,
            on_rider_enter_event: None,
            on_rider_exit_event: None,
            passenger_max_width: Some(0f32),
            pull_in_entities: Some(false),
            rider_can_interact: Some(false),
            seat_count: Some(1i32),
            seats: None,
        }
    }
}
