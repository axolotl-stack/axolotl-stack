use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:exhaustion_values`. Defines how much exhaustion each player action should take.
#[derive(Component, Debug, Clone, PartialEq)]
#[component(storage = "SparseSet")]
pub struct ExhaustionValues {
    /// attack
    pub attack: Option<f32>,
    /// damage
    pub damage: Option<f32>,
    /// heal
    pub heal: Option<f32>,
    /// jump
    pub jump: Option<f32>,
    /// lunge
    pub lunge: Option<f32>,
    /// mine
    pub mine: Option<f32>,
    /// sprint
    pub sprint: Option<f32>,
    /// sprint_jump
    pub sprint_jump: Option<f32>,
    /// swim
    pub swim: Option<f32>,
    /// walk
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
