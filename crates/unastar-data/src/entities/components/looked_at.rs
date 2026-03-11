use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct LookedAtLookAtLocations {
    ///Location to be looked at
    pub location: Option<String>,
    ///Vertical offset from the set location
    pub vertical_offset: Option<f32>,
}
impl Default for LookedAtLookAtLocations {
    fn default() -> Self {
        Self {
            location: None,
            vertical_offset: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct LookedAtLookedAtEvent {
    ///The event to fire.
    pub event: Option<String>,
    ///filters
    pub filters: Option<crate::types::BedrockValue>,
    ///The target of the event.
    pub target: Option<String>,
}
impl Default for LookedAtLookedAtEvent {
    fn default() -> Self {
        Self {
            event: None,
            filters: None,
            target: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct LookedAtNotLookedAtEvent {
    ///The event to fire.
    pub event: Option<String>,
    ///filters
    pub filters: Option<crate::types::BedrockValue>,
    ///The target of the event.
    pub target: Option<String>,
}
impl Default for LookedAtNotLookedAtEvent {
    fn default() -> Self {
        Self {
            event: None,
            filters: None,
            target: None,
        }
    }
}
/// Bedrock component `minecraft:looked_at`. Defines the behavior when another entity looks at this entity.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct LookedAt {
    ///Defines, in degrees, the width of the field of view for entities looking at the owner entity. If 'scale_fov_by_distance' is set to true, this value corresponds to the field of view at a distance of one block between the entities.
    pub field_of_view: Option<f32>,
    ///Defines the entities that can trigger this component.
    pub filters: Option<crate::types::BedrockValue>,
    ///Limits the search to only the nearest Player that meets the specified "filters" rather than all nearby entities.
    pub find_players_only: Option<bool>,
    ///Defines the type of block shape used to check for line of sight obstructions.
    pub line_of_sight_obstruction_type: Option<String>,
    ///A list of locations on the owner entity towards which line of sight checks are performed. At least one location must be unobstructed for the entity to be considered as looked at.
    pub look_at_locations: Option<Vec<LookedAtLookAtLocations>>,
    ///The range for the random amount of time during which the entity is `cooling down` and won't get angered or look for a target.
    pub looked_at_cooldown: Option<crate::types::RangeOrVal<f32>>,
    ///The event identifier to run when the entities specified in filters look at this entity.
    pub looked_at_event: Option<LookedAtLookedAtEvent>,
    ///Defines the minimum, continuous time the owner entity has to be looked at before being considered as such.
    pub min_looked_at_duration: Option<f32>,
    ///Defines the event to trigger when no entity is found looking at the owner entity.
    pub not_looked_at_event: Option<LookedAtNotLookedAtEvent>,
    ///When true, the field of view narrows as the distance between the owner entity and the entity looking at it increases. This ensures that the width of the view cone remains somewhat constant towards the owner entity position, regardless of distance.
    pub scale_fov_by_distance: Option<bool>,
    ///Maximum distance this entity will look for another entity looking at it.
    pub search_radius: Option<f32>,
    /**Defines if and how the owner entity will set entities that are looking at it as its combat targets. Valid values:
    - "never", looking entities are never set as targets, but events are emitted.
    - "once_and_stop_scanning", the first detected looking entity is set as target. Scanning and event emission is suspended if and until the owner entity has a target.
    - "once_and_keep_scanning", the first detected looking entity is set as target. Scanning and event emission continues.*/
    pub set_target: Option<String>,
}
impl Default for LookedAt {
    fn default() -> Self {
        Self {
            field_of_view: Some(26f32),
            filters: None,
            find_players_only: Some(false),
            line_of_sight_obstruction_type: Some("collision".to_string()),
            look_at_locations: None,
            looked_at_cooldown: None,
            looked_at_event: None,
            min_looked_at_duration: Some(0f32),
            not_looked_at_event: None,
            scale_fov_by_distance: Some(true),
            search_radius: Some(10f32),
            set_target: Some("false".to_string()),
        }
    }
}
