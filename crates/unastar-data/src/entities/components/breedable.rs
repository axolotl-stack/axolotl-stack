use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BreedableBreedsWithBreedEvent {
    ///The event to fire.
    pub event: Option<String>,
    ///filters
    pub filters: Option<crate::types::BedrockValue>,
    ///The target of the event.
    pub target: Option<String>,
}
impl Default for BreedableBreedsWithBreedEvent {
    fn default() -> Self {
        Self {
            event: None,
            filters: None,
            target: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BreedableBreedsWith {
    ///The entity definition of this entity's babies.
    pub baby_type: Option<String>,
    ///Event to run when this entity breeds.
    pub breed_event: Option<BreedableBreedsWithBreedEvent>,
    ///The entity definition of this entity's mate.
    pub mate_type: Option<String>,
}
impl Default for BreedableBreedsWith {
    fn default() -> Self {
        Self {
            baby_type: None,
            breed_event: None,
            mate_type: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BreedableEnvironmentRequirements {
    ///The block types required nearby for the entity to breed.
    pub blocks: Option<Vec<crate::types::BedrockValue>>,
    ///The number of the required block types nearby for the entity to breed.
    pub count: Option<f32>,
    ///How many blocks radius from the mob's center to search in for the required blocks. Bounded between 0 and 16.
    pub radius: Option<f32>,
}
impl Default for BreedableEnvironmentRequirements {
    fn default() -> Self {
        Self {
            blocks: None,
            count: None,
            radius: None,
        }
    }
}
/// Bedrock component `minecraft:breedable`. Defines the way an entity can get into the `love` state.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct Breedable {
    ///If true, entities can breed while sitting.
    pub allow_sitting: Option<bool>,
    ///Time in seconds before the Entity can breed again.
    pub breed_cooldown: Option<f32>,
    ///The list of items that can be used to get the entity into the `love` state.
    pub breed_items: Option<Vec<crate::types::BedrockValue>>,
    ///The list of entity definitions that this entity can breed with.
    pub breeds_with: Option<Vec<BreedableBreedsWith>>,
    ///If true, the entity will become pregnant instead of spawning a baby.
    pub causes_pregnancy: Option<bool>,
    ///The list of nearby block requirements to get the entity into the `love` state.
    pub environment_requirements: Option<Vec<BreedableEnvironmentRequirements>>,
    ///Chance that up to 16 babies will spawn between 0.0 and 1.0, where 1.0 is 100%.
    pub extra_baby_chance: Option<f32>,
    ///The filters to run when attempting to fall in love.
    pub love_filters: Option<crate::types::BedrockValue>,
    ///If true, the entity needs to be at full health before it can breed.
    pub require_full_health: Option<bool>,
    ///If true, the entities need to be tamed first before they can breed.
    pub require_tame: Option<bool>,
}
impl Default for Breedable {
    fn default() -> Self {
        Self {
            allow_sitting: Some(false),
            breed_cooldown: Some(60f32),
            breed_items: None,
            breeds_with: None,
            causes_pregnancy: Some(false),
            environment_requirements: None,
            extra_baby_chance: Some(0f32),
            love_filters: None,
            require_full_health: Some(false),
            require_tame: Some(true),
        }
    }
}
