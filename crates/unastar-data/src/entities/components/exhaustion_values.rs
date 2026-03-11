use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:exhaustion_values`. Defines how much exhaustion each player action should take.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct ExhaustionValues {
    ///Amount of exhaustion applied when attacking.
    pub attack: Option<f32>,
    ///Amount of exhaustion applied when taking damage.
    pub damage: Option<f32>,
    ///Amount of exhaustion applied when healed through food regeneration.
    pub heal: Option<f32>,
    ///Amount of exhaustion applied when jumping.
    pub jump: Option<f32>,
    ///Amount of exhaustion applied when triggering the lunge enchantment, multiplied by the enchantment level.
    pub lunge: Option<f32>,
    ///Amount of exhaustion applied when mining.
    pub mine: Option<f32>,
    ///Amount of exhaustion applied when sprinting.
    pub sprint: Option<f32>,
    ///Amount of exhaustion applied when sprint jumping.
    pub sprint_jump: Option<f32>,
    ///Amount of exhaustion applied when swimming.
    pub swim: Option<f32>,
    ///Amount of exhaustion applied when walking.
    pub walk: Option<f32>,
}
impl Default for ExhaustionValues {
    fn default() -> Self {
        Self {
            attack: Some(0.1f32),
            damage: Some(0.1f32),
            heal: Some(6f32),
            jump: Some(0.05f32),
            lunge: Some(4f32),
            mine: Some(0.005f32),
            sprint: Some(0.01f32),
            sprint_jump: Some(0.2f32),
            swim: Some(0.01f32),
            walk: Some(0f32),
        }
    }
}
