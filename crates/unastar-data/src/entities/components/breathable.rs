use bevy_ecs::prelude::*;
/// Bedrock component `minecraft:breathable`
#[derive(Component, Debug, Clone, PartialEq)]
pub struct Breathable {
    /// total_supply
    pub total_supply: i32,
    /// suffocate_time
    pub suffocate_time: i32,
    /// breathes_air
    pub breathes_air: bool,
    /// breathes_water
    pub breathes_water: bool,
    /// breathes_lava
    pub breathes_lava: bool,
    /// breathes_solids
    pub breathes_solids: bool,
    /// generates_bubbles
    pub generates_bubbles: bool,
}
impl Default for Breathable {
    fn default() -> Self {
        Self {
            total_supply: 15i32,
            suffocate_time: -1i32,
            breathes_air: true,
            breathes_water: false,
            breathes_lava: false,
            breathes_solids: false,
            generates_bubbles: true,
        }
    }
}
