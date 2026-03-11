use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:home`. Saves a home pos for when the the entity is spawned.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct Home {
    ///Optional block list that the home position will be associated with. If any of the blocks no longer exist at that position, the home restriction is removed. Example syntax: minecraft:sand. Not supported: minecraft:sand:1
    pub home_block_list: Option<Vec<String>>,
    ///The radius that the entity will be restricted to in relation to its home.
    pub restriction_radius: Option<i32>,
    /**Defines how the the entity will be restricted to its home position. The possible values are:
    - 'none', which poses no restriction.
    - 'random_movement', which restricts randomized movement to be around the home position.
    - 'all_movement', which restricts any kind of movement to be around the home position. However, entities that somehow got too far away from their home will always be able to move closer to it, if prompted to do so.*/
    pub restriction_type: Option<String>,
}
impl Default for Home {
    fn default() -> Self {
        Self {
            home_block_list: None,
            restriction_radius: Some(0i32),
            restriction_type: Some("none".to_string()),
        }
    }
}
