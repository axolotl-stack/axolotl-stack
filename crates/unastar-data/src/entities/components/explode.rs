use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:explode`. Defines how the entity explodes.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct Explode {
    ///If true, the explosion will affect blocks and entities under water.
    pub allow_underwater: Option<bool>,
    ///If true, the explosion will destroy blocks in the explosion radius.
    pub breaks_blocks: Option<bool>,
    ///If true, blocks in the explosion radius will be set on fire.
    pub causes_fire: Option<bool>,
    ///A scale factor applied to the explosion's damage to entities. A value of 0 prevents the explosion from dealing any damage. Negative values cause the explosion to heal entities instead.
    pub damage_scaling: Option<f32>,
    ///If true, whether the explosion breaks blocks is affected by the mob griefing game rule.
    pub destroy_affected_by_griefing: Option<bool>,
    ///If true, whether the explosion causes fire is affected by the mob griefing game rule.
    pub fire_affected_by_griefing: Option<bool>,
    ///The range for the random amount of time the fuse will be lit before exploding, a negative value means the explosion will be immediate.
    pub fuse_length: Option<crate::types::RangeOrVal<f32>>,
    ///If true, the fuse is already lit when this component is added to the entity.
    pub fuse_lit: Option<bool>,
    ///A scale factor applied to the knockback force caused by the explosion.
    pub knockback_scaling: Option<f32>,
    ///A blocks explosion resistance will be capped at this value when an explosion occurs.
    pub max_resistance: Option<f32>,
    ///Defines whether the explosion should apply fall damage negation to Players above the point of collision.
    pub negates_fall_damage: Option<bool>,
    ///The name of the particle effect to use.
    pub particle_effect: Option<String>,
    ///The radius of the explosion in blocks and the amount of damage the explosion deals.
    pub power: Option<f32>,
    ///The name of the sound effect played when the explosion triggers.
    pub sound_effect: Option<String>,
    ///If true, the explosion will toggle blocks in the explosion radius.
    pub toggles_blocks: Option<bool>,
}
impl Default for Explode {
    fn default() -> Self {
        Self {
            allow_underwater: Some(false),
            breaks_blocks: Some(true),
            causes_fire: Some(false),
            damage_scaling: Some(1f32),
            destroy_affected_by_griefing: Some(false),
            fire_affected_by_griefing: Some(false),
            fuse_length: None,
            fuse_lit: Some(false),
            knockback_scaling: Some(1f32),
            max_resistance: Some(340282000000000000000000000000000000000f32),
            negates_fall_damage: Some(false),
            particle_effect: Some("explosion".to_string()),
            power: Some(3f32),
            sound_effect: Some("explode".to_string()),
            toggles_blocks: Some(false),
        }
    }
}
