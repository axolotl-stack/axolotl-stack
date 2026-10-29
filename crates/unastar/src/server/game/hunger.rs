//! Player food: exhaustion, natural regeneration and starvation.
//!
//! Rules (survival research §2.2; parameters from the vanilla `player.json`):
//!
//! * creative and spectator players are skipped;
//! * at 4 exhaustion, 4 is removed and one saturation point drained, or one
//!   food point when saturation is empty (never food on peaceful); at most
//!   one step per tick;
//! * on peaceful, food refills by 1 every 10 ticks;
//! * with the `naturalregeneration` game rule, food >= 18 and health below
//!   max, the player heals 1 every 80 ticks and gains
//!   `exhaustion_values.heal` exhaustion (no saturation fast-heal, unlike
//!   Java Edition);
//! * at food 0 the player takes 1 starvation damage every 80 ticks while
//!   health is above 10 (easy), above 1 (normal), or always (hard);
//! * otherwise the 80-tick timer resets every tick.

use bevy_ecs::prelude::*;
use jolyne::valentine::types::{EnumsSharedTypesLegacyDifficulty, GameRuleRuleValue};
use serde::Deserialize;
use unastar_data::actor_components::body::ActorDamageCause;

use crate::entity::components::{GameMode, Health, Hunger};
use crate::entity::mob::combat::{Hit, hurt_mob};
use crate::entity::mob::components::KnockbackPower;

/// Ticks between natural regeneration or starvation steps.
pub const FOOD_TICK_INTERVAL: u32 = 80;
/// Food level needed to regenerate.
pub const REGENERATION_FOOD_LEVEL: i32 = 18;

/// `minecraft:exhaustion_values` of the vanilla player definition.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExhaustionValues {
    pub heal: f32,
    pub jump: f32,
    pub sprint_jump: f32,
    pub mine: f32,
    pub attack: f32,
    pub damage: f32,
    pub walk: f32,
    pub sprint: f32,
    pub swim: f32,
    pub lunge: f32,
}

impl ExhaustionValues {
    /// The values the vanilla `minecraft:player` definition authors.
    pub fn vanilla() -> Self {
        let player = unastar_data::actor_definitions::get("minecraft:player")
            .expect("vanilla player definition");
        let body = player
            .components
            .get("minecraft:exhaustion_values")
            .expect("vanilla player.json has minecraft:exhaustion_values");
        serde_json::from_value(body.clone()).expect("typed minecraft:exhaustion_values")
    }
}

impl Default for ExhaustionValues {
    fn default() -> Self {
        Self::vanilla()
    }
}

/// Server-only food timers of a player.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FoodTimer {
    pub ticks: u32,
    pub regeneration: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Difficulty {
    Peaceful,
    Easy,
    Normal,
    Hard,
}

/// The rules the food tick reads from the announced level settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FoodRules {
    pub difficulty: Difficulty,
    pub natural_regeneration: bool,
}

impl FoodRules {
    pub fn of(world: &World) -> Self {
        let Some(template) = world.get_resource::<super::types::ServerWorldTemplate>() else {
            return Self {
                difficulty: Difficulty::Normal,
                natural_regeneration: true,
            };
        };
        let settings = &template.0.start_game_template.settings;
        let difficulty = match settings.game_difficulty {
            EnumsSharedTypesLegacyDifficulty::Peaceful => Difficulty::Peaceful,
            EnumsSharedTypesLegacyDifficulty::Easy => Difficulty::Easy,
            EnumsSharedTypesLegacyDifficulty::Hard => Difficulty::Hard,
            _ => Difficulty::Normal,
        };
        // An unannounced rule has its vanilla default (true).
        let natural_regeneration = settings
            .rule_data
            .rules_list
            .iter()
            .find(|rule| rule.rule_name.eq_ignore_ascii_case("naturalregeneration"))
            .is_none_or(|rule| !matches!(rule.rule_value, GameRuleRuleValue::Bool(false)));
        Self {
            difficulty,
            natural_regeneration,
        }
    }
}

/// One exhaustion step.
pub fn tick_exhaustion(hunger: &mut Hunger, difficulty: Difficulty) {
    if hunger.exhaustion < Hunger::EXHAUSTION_PER_POINT {
        return;
    }
    hunger.exhaustion -= Hunger::EXHAUSTION_PER_POINT;
    if hunger.saturation > 0.0 {
        hunger.saturation = (hunger.saturation - 1.0).max(0.0);
    } else if difficulty != Difficulty::Peaceful {
        hunger.food_level = (hunger.food_level - 1).max(0);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FoodTick {
    Nothing,
    Heal,
    Starve,
}

/// One food tick: peaceful refill, then natural regeneration or starvation.
pub fn tick_food(
    hunger: &mut Hunger,
    timer: &mut FoodTimer,
    health: &Health,
    rules: FoodRules,
) -> FoodTick {
    timer.ticks = timer.ticks.wrapping_add(1);
    if rules.difficulty == Difficulty::Peaceful && timer.ticks.is_multiple_of(10) {
        hunger.food_level = (hunger.food_level + 1).min(Hunger::MAX_FOOD);
    }
    // The health attribute is compared as whole points.
    let current = health.current.ceil() as i32;
    let max = health.max as i32;
    let regenerates = rules.natural_regeneration
        && hunger.food_level >= REGENERATION_FOOD_LEVEL
        && current > 0
        && current < max;
    if !regenerates && hunger.food_level > 0 {
        timer.regeneration = 0;
        return FoodTick::Nothing;
    }
    timer.regeneration += 1;
    if timer.regeneration < FOOD_TICK_INTERVAL {
        return FoodTick::Nothing;
    }
    timer.regeneration = 0;
    if regenerates {
        return FoodTick::Heal;
    }
    let starves = match rules.difficulty {
        Difficulty::Hard => true,
        Difficulty::Normal => current > 1,
        Difficulty::Easy | Difficulty::Peaceful => current > 10,
    };
    if starves {
        FoodTick::Starve
    } else {
        FoodTick::Nothing
    }
}

/// System (exclusive): tick every survival or adventure player's food.
/// Starvation goes through the shared damage path ([`hurt_mob`]).
pub fn tick_hunger(
    world: &mut World,
    players: &mut QueryState<(Entity, &mut Hunger, &mut FoodTimer, &mut Health, &GameMode)>,
    mut starving: Local<Vec<Entity>>,
) {
    let rules = FoodRules::of(world);
    let heal = world.get_resource_or_init::<ExhaustionValues>().heal;
    for (entity, mut hunger, mut timer, mut health, mode) in players.iter_mut(world) {
        if matches!(mode, GameMode::Creative | GameMode::Spectator) || health.is_dead() {
            continue;
        }
        let mut next = hunger.clone();
        tick_exhaustion(&mut next, rules.difficulty);
        match tick_food(&mut next, &mut timer, &health, rules) {
            FoodTick::Heal => {
                next.exhaust(heal);
                health.heal(1.0);
            }
            FoodTick::Starve => starving.push(entity),
            FoodTick::Nothing => {}
        }
        if next.food_level != hunger.food_level
            || next.saturation != hunger.saturation
            || next.exhaustion != hunger.exhaustion
        {
            *hunger = next;
        }
    }
    for entity in starving.drain(..) {
        hurt_mob(
            world,
            entity,
            Hit {
                damage: 1.0,
                cause: ActorDamageCause::Starve,
                attacker: None,
                by_player: false,
                knockback_from: None,
                knockback_power: KnockbackPower::DEFAULT,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hunger(food: i32, saturation: f32, exhaustion: f32) -> Hunger {
        Hunger {
            food_level: food,
            saturation,
            exhaustion,
        }
    }

    fn rules(difficulty: Difficulty) -> FoodRules {
        FoodRules {
            difficulty,
            natural_regeneration: true,
        }
    }

    #[test]
    fn exhaustion_values_come_from_vanilla_player_json() {
        let values = ExhaustionValues::vanilla();
        assert_eq!(
            (values.heal, values.jump, values.sprint_jump, values.mine),
            (6.0, 0.05, 0.2, 0.005)
        );
        assert_eq!(
            (
                values.attack,
                values.damage,
                values.sprint,
                values.swim,
                values.walk
            ),
            (0.1, 0.1, 0.1, 0.01, 0.0)
        );
    }

    #[test]
    fn exhaustion_drains_saturation_before_food_one_step_per_tick() {
        let mut h = hunger(20, 1.0, 9.0);
        tick_exhaustion(&mut h, Difficulty::Normal);
        assert_eq!((h.food_level, h.saturation, h.exhaustion), (20, 0.0, 5.0));
        tick_exhaustion(&mut h, Difficulty::Normal);
        assert_eq!((h.food_level, h.saturation, h.exhaustion), (19, 0.0, 1.0));
        tick_exhaustion(&mut h, Difficulty::Normal);
        assert_eq!((h.food_level, h.exhaustion), (19, 1.0));
    }

    #[test]
    fn peaceful_never_drains_food_and_refills_every_ten_ticks() {
        let mut h = hunger(10, 0.0, 4.0);
        tick_exhaustion(&mut h, Difficulty::Peaceful);
        assert_eq!((h.food_level, h.exhaustion), (10, 0.0));
        let health = Health::new(20.0);
        let mut timer = FoodTimer::default();
        for _ in 0..9 {
            tick_food(&mut h, &mut timer, &health, rules(Difficulty::Peaceful));
        }
        assert_eq!(h.food_level, 10);
        tick_food(&mut h, &mut timer, &health, rules(Difficulty::Peaceful));
        assert_eq!(h.food_level, 11);
    }

    #[test]
    fn regeneration_heals_every_80_ticks_at_18_food_only() {
        let damaged = Health::with_current(10.0, 20.0);
        let heals = |food: i32, health: &Health, natural: bool| {
            let mut h = hunger(food, 0.0, 0.0);
            let mut timer = FoodTimer::default();
            let rules = FoodRules {
                difficulty: Difficulty::Normal,
                natural_regeneration: natural,
            };
            (0..160)
                .filter(|_| tick_food(&mut h, &mut timer, health, rules) == FoodTick::Heal)
                .count()
        };
        assert_eq!(heals(18, &damaged, true), 2);
        assert_eq!(heals(17, &damaged, true), 0);
        assert_eq!(heals(20, &damaged, false), 0);
        assert_eq!(heals(20, &Health::new(20.0), true), 0);
    }

    #[test]
    fn starvation_floor_depends_on_difficulty() {
        let starve = |difficulty, health: f32| {
            let mut h = hunger(0, 0.0, 0.0);
            let mut timer = FoodTimer::default();
            let health = Health::with_current(health, 20.0);
            (0..80)
                .map(|_| tick_food(&mut h, &mut timer, &health, rules(difficulty)))
                .last()
                .unwrap()
        };
        assert_eq!(starve(Difficulty::Easy, 11.0), FoodTick::Starve);
        assert_eq!(starve(Difficulty::Easy, 10.0), FoodTick::Nothing);
        assert_eq!(starve(Difficulty::Normal, 2.0), FoodTick::Starve);
        assert_eq!(starve(Difficulty::Normal, 1.0), FoodTick::Nothing);
        assert_eq!(starve(Difficulty::Hard, 1.0), FoodTick::Starve);
    }

    #[test]
    fn eating_adds_nutrition_and_saturation_capped_at_food() {
        let mut h = hunger(10, 0.0, 0.0);
        h.eat(8, 0.8);
        assert_eq!(h.food_level, 18);
        assert!((h.saturation - 12.8).abs() < 1e-5);
        let mut full = hunger(19, 18.0, 0.0);
        full.eat(8, 0.8);
        assert_eq!((full.food_level, full.saturation), (20, 20.0));
    }
}
