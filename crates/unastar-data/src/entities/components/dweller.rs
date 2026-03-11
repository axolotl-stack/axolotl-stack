use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:dweller`. Allows a mob to join and migrate between villages and other dwellings.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct Dweller {
    /// can_find_poi
    pub can_find_poi: Option<bool>,
    /// can_migrate
    pub can_migrate: Option<bool>,
    /// dweller_role
    pub dweller_role: Option<String>,
    /// dwelling_bounds_tolerance
    pub dwelling_bounds_tolerance: Option<f32>,
    /// dwelling_type
    pub dwelling_type: Option<String>,
    /// first_founding_reward
    pub first_founding_reward: Option<i32>,
    /// preferred_profession
    pub preferred_profession: Option<String>,
    /// update_interval_base
    pub update_interval_base: Option<f32>,
    /// update_interval_variant
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
