use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct DespawnDespawnFromDistance {
    ///Maximum distance for standard despawn rules to instantly despawn the mob.
    pub max_distance: Option<i32>,
    ///Minimum distance for standard despawn rules to try to despawn the mob.
    pub min_distance: Option<i32>,
}
impl Default for DespawnDespawnFromDistance {
    fn default() -> Self {
        Self {
            max_distance: None,
            min_distance: None,
        }
    }
}
/// Bedrock component `minecraft:despawn`. Despawns the Actor when the despawn rules or optional filters evaluate to true.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct Despawn {
    ///Determines if `min_range_random_chance` is used in the standard despawn rules.
    pub despawn_from_chance: Option<bool>,
    ///Defines the minimum and maximum distance for despawn to occur.
    pub despawn_from_distance: Option<DespawnDespawnFromDistance>,
    ///Determines if the `min_range_inactivity_timer` is used in the standard despawn rules.
    pub despawn_from_inactivity: Option<bool>,
    ///Determines if the mob is instantly despawned at the edge of simulation distance in the standard despawn rules.
    pub despawn_from_simulation_edge: Option<bool>,
    ///The list of conditions that must be satisfied before the Actor is despawned. If a filter is defined then standard despawn rules are ignored.
    pub filters: Option<crate::types::BedrockValue>,
    ///The amount of time in seconds that the mob must be inactive.
    pub min_range_inactivity_timer: Option<i32>,
    ///A random chance between 1 and the given value.
    pub min_range_random_chance: Option<i32>,
    ///If true, all entities linked to this entity in a child relationship (eg. leashed) will also be despawned.
    pub remove_child_entities: Option<bool>,
}
impl Default for Despawn {
    fn default() -> Self {
        Self {
            despawn_from_chance: Some(true),
            despawn_from_distance: None,
            despawn_from_inactivity: Some(true),
            despawn_from_simulation_edge: Some(true),
            filters: None,
            min_range_inactivity_timer: Some(30i32),
            min_range_random_chance: Some(800i32),
            remove_child_entities: Some(false),
        }
    }
}
