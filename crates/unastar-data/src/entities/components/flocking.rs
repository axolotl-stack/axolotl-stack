use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:flocking`. Allows entities to flock in groups in water or not.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct Flocking {
    ///The amount of blocks away the entity will look at to push away from.
    pub block_distance: Option<f32>,
    ///The weight of the push back away from blocks.
    pub block_weight: Option<f32>,
    ///The amount of push back given to a flocker that breaches out of the water.
    pub breach_influence: Option<f32>,
    ///The threshold in which to start applying cohesion.
    pub cohesion_threshold: Option<f32>,
    ///The weight applied for the cohesion steering of the flock.
    pub cohesion_weight: Option<f32>,
    ///The weight on which to apply on the goal output.
    pub goal_weight: Option<f32>,
    ///Determines the high bound amount of entities that can be allowed in the flock.
    pub high_flock_limit: Option<i32>,
    ///Tells the Flocking Component if the entity exists in water.
    pub in_water: Option<bool>,
    ///The area around the entity that allows others to be added to the flock.
    pub influence_radius: Option<f32>,
    ///The distance in which the flocker will stop applying cohesion.
    pub innner_cohesion_threshold: Option<f32>,
    ///The percentage chance between 0-1 that a fish will spawn and not want to join flocks. Invalid values will be capped at the end points.
    pub loner_chance: Option<f32>,
    ///Determines the low bound amount of entities that can be allowed in the flock.
    pub low_flock_limit: Option<i32>,
    ///Tells the flockers that they can only match similar entities that also match the variant, mark variants, and color data of the other potential flockers.
    pub match_variants: Option<bool>,
    ///The Maximum height allowable in the air or water.
    pub max_height: Option<f32>,
    ///The Minimum height allowable in the air or water.
    pub min_height: Option<f32>,
    ///The distance that is determined to be to close to another flocking and to start applying separation.
    pub separation_threshold: Option<f32>,
    ///The weight applied to the separation of the flock.
    pub separation_weight: Option<f32>,
    ///Tells the flockers that they will follow flocks based on the center of mass.
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
