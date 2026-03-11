use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:breathable`. Defines what blocks this entity can breathe in and gives them the ability to suffocate.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct Breathable {
    ///List of blocks this entity can breathe in, in addition to the above.
    pub breathe_blocks: Option<Vec<crate::types::BedrockValue>>,
    ///If true, this entity can breathe in air.
    pub breathes_air: Option<bool>,
    ///If true, this entity can breathe in lava.
    pub breathes_lava: Option<bool>,
    ///If true, this entity can breathe in solid blocks.
    pub breathes_solids: Option<bool>,
    ///If true, this entity can breathe in water.
    pub breathes_water: Option<bool>,
    ///If true, water-only breathers will take Dehydration damage when out of water.
    pub can_dehydrate: Option<bool>,
    ///If true, this entity will have visible bubbles while in water.
    pub generates_bubbles: Option<bool>,
    ///Time in seconds to recover breath to maximum.
    pub inhale_time: Option<f32>,
    ///List of blocks this entity can't breathe in, in addition to the above.
    pub non_breathe_blocks: Option<Vec<crate::types::BedrockValue>>,
    ///Time in seconds between suffocation damage.
    pub suffocate_time: Option<i32>,
    ///Time in seconds the entity can hold its breath.
    pub total_supply: Option<i32>,
}
impl Default for Breathable {
    fn default() -> Self {
        Self {
            breathe_blocks: None,
            breathes_air: Some(true),
            breathes_lava: Some(false),
            breathes_solids: Some(false),
            breathes_water: Some(false),
            can_dehydrate: Some(false),
            generates_bubbles: Some(true),
            inhale_time: Some(0f32),
            non_breathe_blocks: None,
            suffocate_time: Some(-1i32),
            total_supply: Some(15i32),
        }
    }
}
