use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:dweller`. Allows a mob to join and migrate between villages and other dwellings.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct Dweller {
    ///Whether or not the mob can find and add POI's to the dwelling.
    pub can_find_poi: Option<bool>,
    ///Can this mob migrate between dwellings? Or does it only have its initial dwelling?.
    pub can_migrate: Option<bool>,
    ///The role of which the mob plays in the dwelling. Current Roles: inhabitant, defender, hostile, passive.
    pub dweller_role: Option<String>,
    ///A padding distance for checking if the mob is within the dwelling.
    pub dwelling_bounds_tolerance: Option<f32>,
    ///The type of dwelling the mob wishes to join. Current Types: village
    pub dwelling_type: Option<String>,
    ///How much reputation should the players be rewarded on first founding?.
    pub first_founding_reward: Option<i32>,
    ///Allows the user to define a starting profession for this particular Dweller, instead of letting them choose organically. (They still need to gain experience from trading before this takes effect.)
    pub preferred_profession: Option<String>,
    ///How often the mob checks on their dwelling status in ticks. Positive values only.
    pub update_interval_base: Option<f32>,
    ///The variant value in ticks that will be added to the update_interval_base.
    pub update_interval_variant: Option<f32>,
}
impl Default for Dweller {
    fn default() -> Self {
        Self {
            can_find_poi: None,
            can_migrate: None,
            dweller_role: None,
            dwelling_bounds_tolerance: None,
            dwelling_type: None,
            first_founding_reward: None,
            preferred_profession: None,
            update_interval_base: None,
            update_interval_variant: None,
        }
    }
}
