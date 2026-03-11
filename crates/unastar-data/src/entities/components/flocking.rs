use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:flocking`. Allows entities to flock in groups in water or not.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct Flocking {
    /// block_distance
    pub block_distance: Option<f32>,
    /// block_weight
    pub block_weight: Option<f32>,
    /// breach_influence
    pub breach_influence: Option<f32>,
    /// cohesion_threshold
    pub cohesion_threshold: Option<f32>,
    /// cohesion_weight
    pub cohesion_weight: Option<f32>,
    /// goal_weight
    pub goal_weight: Option<f32>,
    /// high_flock_limit
    pub high_flock_limit: Option<i32>,
    /// in_water
    pub in_water: Option<bool>,
    /// influence_radius
    pub influence_radius: Option<f32>,
    /// innner_cohesion_threshold
    pub innner_cohesion_threshold: Option<f32>,
    /// loner_chance
    pub loner_chance: Option<f32>,
    /// low_flock_limit
    pub low_flock_limit: Option<i32>,
    /// match_variants
    pub match_variants: Option<bool>,
    /// max_height
    pub max_height: Option<f32>,
    /// min_height
    pub min_height: Option<f32>,
    /// separation_threshold
    pub separation_threshold: Option<f32>,
    /// separation_weight
    pub separation_weight: Option<f32>,
    /// use_center_of_mass
    pub use_center_of_mass: Option<bool>,
}
impl Default for Flocking {
    fn default() -> Self {
        Self {
            block_distance: Some(0f32),
            block_weight: Some(0f32),
            breach_influence: Some(0f32),
            cohesion_threshold: Some(1f32),
            cohesion_weight: Some(1f32),
            goal_weight: Some(0f32),
            high_flock_limit: Some(0i32),
            in_water: Some(false),
            influence_radius: Some(0f32),
            innner_cohesion_threshold: Some(0f32),
            loner_chance: Some(0f32),
            low_flock_limit: Some(0i32),
            match_variants: Some(false),
            max_height: Some(0f32),
            min_height: Some(0f32),
            separation_threshold: Some(2f32),
            separation_weight: Some(1f32),
            use_center_of_mass: Some(false),
        }
    }
}
