use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:shooter`. Defines the entity's ranged attack behavior.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct Shooter {
    ///ID of the Potion effect to be applied on hit.
    pub aux_val: Option<i32>,
    ///Actor definition to use as projectile for the ranged attack. The actor definition must have the projectile component to be able to be shot as a projectile
    pub def: Option<String>,
    ///Sets whether the projectiles being used are flagged as magic. If set, the ranged attack goal will not be used at the same time as other magic goals, such as minecraft:behavior.drink_potion.
    pub magic: Option<bool>,
    ///Velocity in which the projectiles will be shot. A power of 0 will be overwritten by the default projectile throw power.
    pub power: Option<f32>,
    ///List of projectiles that can be used by the shooter. Projectiles are evaluated in the order of the list; after a projectile is chosen, the rest of the list is ignored.
    pub projectiles: Option<Vec<crate::types::BedrockValue>>,
    ///Sound that is played when the shooter shoots a projectile.
    pub sound: Option<String>,
}
impl Default for Shooter {
    fn default() -> Self {
        Self {
            aux_val: Some(-1i32),
            def: None,
            magic: Some(false),
            power: Some(0f32),
            projectiles: None,
            sound: None,
        }
    }
}
