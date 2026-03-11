use bevy_ecs::prelude::*;
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorPanicPriority {}
impl Default for BehaviorPanicPriority {
    fn default() -> Self {
        Self {}
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorPanicSoundInterval {
    ///The maximum time in seconds before the `panic_sound` plays.
    pub range_max: Option<f32>,
    ///The minimum time in seconds before the `panic_sound` plays.
    pub range_min: Option<f32>,
}
impl Default for BehaviorPanicSoundInterval {
    fn default() -> Self {
        Self {
            range_max: None,
            range_min: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BehaviorPanicSpeedMultiplier {}
impl Default for BehaviorPanicSpeedMultiplier {
    fn default() -> Self {
        Self {}
    }
}
/// Bedrock component `minecraft:behavior.panic`. Allows the mob to enter the panic state, which makes it run around and away from the damage source that made it enter this state.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct BehaviorPanic {
    ///The list of Entity Damage Sources that will cause this mob to panic.
    pub damage_sources: Option<Vec<String>>,
    ///If true, this mob will not stop panicking until it can't move anymore or the goal is removed from it.
    pub force: Option<bool>,
    ///If true, the mob will not panic in response to damage from other mobs. This overrides the damage types in `damage_sources`
    pub ignore_mob_damage: Option<bool>,
    ///The sound event to play when this mob is in panic.
    pub panic_sound: Option<String>,
    ///If true, the mob will prefer water over land.
    pub prefer_water: Option<bool>,
    ///priority
    pub priority: Option<BehaviorPanicPriority>,
    ///The range of time in seconds to randomly wait before playing the sound again.
    pub sound_interval: Option<BehaviorPanicSoundInterval>,
    ///speed_multiplier
    pub speed_multiplier: Option<BehaviorPanicSpeedMultiplier>,
}
impl Default for BehaviorPanic {
    fn default() -> Self {
        Self {
            damage_sources: Some(
                vec![
                    "[campfire, fire, fire_tick, freezing, lava, lightning, magma, soul_campfire, temperature, entity_attack, entity_explosion, fireworks, magic, projectile, ram_attack, sonic_boom, wither, mace_smash]"
                    .to_string()
                ],
            ),
            force: Some(false),
            ignore_mob_damage: Some(false),
            panic_sound: None,
            prefer_water: Some(false),
            priority: None,
            sound_interval: None,
            speed_multiplier: Some(BehaviorPanicSpeedMultiplier {}),
        }
    }
}
