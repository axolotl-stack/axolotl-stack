use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct DamageSensorTriggers {
    ///Type of damage that triggers the events.
    pub cause: Option<String>,
    ///A modifier that adds to/removes from the base damage from the damage cause. It does not reduce damage to less than 0.
    pub damage_modifier: Option<f32>,
    ///A multiplier that modifies the base damage from the damage cause. If deals_damage is true the multiplier can only reduce the damage the entity will take to a minimum of 1.
    pub damage_multiplier: Option<f32>,
    /**Defines how received damage affects the entity:
    - 'yes', received damage is applied to the entity.
    - 'no', received damage is not applied to the entity.
    - 'no_but_side_effects_apply', received damage is not applied to the entity, but the side effects of the attack are. This means that the attacker's weapon loses durability, enchantment side effects are applied, and so on.*/
    pub deals_damage: Option<String>,
    ///Specifies filters for entity definitions and events.
    pub on_damage: Option<crate::types::BedrockValue>,
    ///Defines what sound to play, if any, when the on_damage filters are met.
    pub on_damage_sound_event: Option<String>,
}
impl Default for DamageSensorTriggers {
    fn default() -> Self {
        Self {
            cause: None,
            damage_modifier: None,
            damage_multiplier: None,
            deals_damage: None,
            on_damage: None,
            on_damage_sound_event: None,
        }
    }
}
/// Bedrock component `minecraft:damage_sensor`. Defines what events to call when this entity is damaged by specific entities or items.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct DamageSensor {
    ///The list of triggers that fire when the environment conditions match the given filter criteria.
    pub triggers: Option<Vec<DamageSensorTriggers>>,
}
impl Default for DamageSensor {
    fn default() -> Self {
        Self { triggers: None }
    }
}
