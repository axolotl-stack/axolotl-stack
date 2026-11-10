//! The player's air supply and its `BREATHING` actor flag, kept as BDS
//! 1.26.51 does (`testdata/bds-1.26.51-breath.json`,
//! `testdata/bds-1.26.51-player-data.json`). The client shows the air
//! bubbles while the flag is off or the air is short of full.
//!
//! Every tick, each live player whose join has handed it over
//! (`PlayStatus(PlayerSpawn)` sent) either holds its breath or breathes:
//! - In a bubble column (any block of its box), it breathes and its air is
//!   full at once.
//! - In survival or adventure, without water breathing or conduit power,
//!   with its eyes under water ([`under_water_at`] its position), it holds
//!   its breath: the flag is off and the air drops by 1, below zero once
//!   it runs out. When it would reach [`BreathRules::drown_at`] it is 0
//!   again and the player takes [`DROWNING_DAMAGE`] as drowning (nothing
//!   with the `drowningdamage` game rule off), so a hit every 20 ticks.
//!   A Respiration helmet of level L spares the air on a tick with chance
//!   L / (L + 1) (the drop and the hits come L + 1 times slower).
//! - Otherwise it breathes, under water too in creative or spectator or
//!   with either effect: the flag is on and the air refills by
//!   [`BreathRules::inhale_per_tick`] up to the full supply. Air below
//!   zero is set to 0 first, without refilling that tick.
//!
//! The air and flag reach the player and its viewers through
//! `actor_data` at the end of the tick. Join sends full air without the
//! flag (whatever air the saved player had), so the first tick after the
//! hand-over turns the flag on and sends the restored air. A respawn
//! refills the air ([`refill`]).

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use unastar_data::actor_components::body::ActorDamageCause;

use super::constants::DROWNING_DAMAGE;
use super::rules::{BreathRules, PlayerRules};
use super::{Dead, hurt_player};
use crate::entity::components::{ArmourInventory, EffectType, Effects, GameMode, Player, Position};
use crate::entity::mob::combat::Hit;
use crate::entity::mob::components::KnockbackPower;
use crate::entity::mob::events::under_water_at;
use crate::entity::systems::BlockSource;
use crate::entity::{EntityRandom, PLAYER_EYE_HEIGHT};
use crate::world::ecs::AwaitingSpawnStatus;

/// Width and height of the box a bubble column must reach (a standing
/// player's).
const BOX: (f64, f64) = (0.6, 1.8);
/// The Bedrock enchantment ID of Respiration (the published enchantment id
/// enum; the `/enchant` enum lists it sixth from 0).
const RESPIRATION: i16 = 6;

/// A player's air supply and `BREATHING` flag.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerBreath {
    /// Air left in ticks; below zero while the player drowns.
    pub air: i16,
    /// The `BREATHING` actor flag.
    pub breathing: bool,
}

/// What a player does with its breath this tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Breath {
    /// Eyes under water it cannot breathe: the air drops.
    Hold,
    /// Eyes under water it cannot breathe, but Respiration spares the air
    /// this tick.
    Spared,
    /// The air refills.
    Breathe,
    /// In a bubble column: the air is full at once.
    Bubbles,
}

impl PlayerBreath {
    /// A joining player: full air, flag off, as its join `SetActorData`
    /// says.
    pub fn joined(rules: &BreathRules) -> Self {
        Self::restored(rules, rules.max_air)
    }

    /// A joining player whose saved air supply was `air` (BDS restores it
    /// after the join packet, which still says full air).
    pub fn restored(rules: &BreathRules, air: i16) -> Self {
        Self {
            air: air.clamp(rules.drown_at + 1, rules.max_air),
            breathing: false,
        }
    }

    /// One tick of `breath`; returns whether a drowning hit lands.
    pub fn tick(&mut self, rules: &BreathRules, breath: Breath) -> bool {
        self.breathing = !matches!(breath, Breath::Hold | Breath::Spared);
        match breath {
            Breath::Spared => {}
            Breath::Hold => {
                self.air -= 1;
                if self.air <= rules.drown_at {
                    self.air = 0;
                    return true;
                }
            }
            Breath::Bubbles => self.air = rules.max_air,
            Breath::Breathe if self.air < 0 => self.air = 0,
            Breath::Breathe => self.air = (self.air + rules.inhale_per_tick).min(rules.max_air),
        }
        false
    }
}

/// Whether a bubble column block overlaps `player`'s box. `None` while a
/// block there is not loaded.
fn in_bubble_column(world: &World, feet: DVec3) -> Option<bool> {
    let Some(column) = world.registry().get_by_name("minecraft:bubble_column") else {
        return Some(false);
    };
    let states = column.min_state_id..=column.max_state_id;
    let half = BOX.0 / 2.0;
    let min = (feet - DVec3::new(half, 0.0, half)).floor().as_ivec3();
    let max = (feet + DVec3::new(half, BOX.1, half)).floor().as_ivec3();
    for y in min.y..=max.y {
        for x in min.x..=max.x {
            for z in min.z..=max.z {
                if states.contains(&world.block_at(IVec3::new(x, y, z))?) {
                    return Some(true);
                }
            }
        }
    }
    Some(false)
}

/// What `player` does with its breath: see the module documentation.
/// `None` while the blocks around it are not loaded.
fn breath_of(world: &World, player: Entity) -> Option<Breath> {
    let eyes = world.get::<Position>(player)?.0;
    if in_bubble_column(world, eyes - DVec3::Y * PLAYER_EYE_HEIGHT)? {
        return Some(Breath::Bubbles);
    }
    if !under_water_at(world, eyes)? {
        return Some(Breath::Breathe);
    }
    let mode = world.get::<GameMode>(player).copied().unwrap_or_default();
    let effect = world.get::<Effects>(player).is_some_and(|effects| {
        effects.has(EffectType::WATER_BREATHING) || effects.has(EffectType::CONDUIT_POWER)
    });
    Some(
        if matches!(mode, GameMode::Survival | GameMode::Adventure) && !effect {
            Breath::Hold
        } else {
            Breath::Breathe
        },
    )
}

/// The Respiration level of the helmet `player` wears.
fn respiration(world: &World, player: Entity) -> u8 {
    world.get::<ArmourInventory>(player).map_or(0, |armour| {
        crate::item::enchant::enchantment_ids(armour.helmet())
            .into_iter()
            .find(|(id, _)| *id == RESPIRATION)
            .map_or(0, |(_, level)| level)
    })
}

/// Whether Respiration of `level` spares the air this tick: chance
/// `level / (level + 1)` (BDS 1.26.51: about 1 in 4 ticks drops the air
/// at level III, 1 in 2 at level I).
fn spares_air(world: &mut World, level: u8) -> bool {
    use rand::Rng;
    level > 0
        && world
            .get_resource_or_init::<EntityRandom>()
            .0
            .gen_range(0..=level)
            > 0
}

/// The players that breathe: alive, their join handed over.
type Breathers = QueryState<
    Entity,
    (
        With<Player>,
        With<PlayerBreath>,
        Without<Dead>,
        Without<AwaitingSpawnStatus>,
    ),
>;

/// System (exclusive, after the environment damage): one breath tick of
/// every player and its drowning hit.
pub fn breathe(world: &mut World, players: &mut Breathers, mut due: Local<Vec<Entity>>) {
    due.extend(players.iter(world));
    if due.is_empty() {
        return;
    }
    let rules = world.resource::<PlayerRules>().breath;
    for player in due.drain(..) {
        let Some(mut breath) = breath_of(world, player) else {
            continue;
        };
        if breath == Breath::Hold {
            let level = respiration(world, player);
            if spares_air(world, level) {
                breath = Breath::Spared;
            }
        }
        let drowned = world
            .get_mut::<PlayerBreath>(player)
            .expect("queried breather")
            .tick(&rules, breath);
        if let Some(mut data) = world.get_mut::<super::actor_data::PlayerActorData>(player) {
            data.ticked();
        }
        if drowned {
            hurt_player(
                world,
                player,
                &Hit {
                    damage: DROWNING_DAMAGE,
                    cause: ActorDamageCause::Drowning,
                    attacker: None,
                    by_player: false,
                    knockback_from: None,
                    knockback_power: KnockbackPower::DEFAULT,
                },
            );
        }
    }
}

/// Refill a respawned player's air.
pub fn refill(world: &mut World, player: Entity) {
    let max_air = world.resource::<PlayerRules>().breath.max_air;
    if let Some(mut breath) = world.get_mut::<PlayerBreath>(player) {
        breath.air = max_air;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RULES: BreathRules = BreathRules {
        max_air: 300,
        inhale_per_tick: 4,
        drown_at: -20,
    };

    fn state(air: i16, breathing: bool) -> PlayerBreath {
        PlayerBreath { air, breathing }
    }

    #[test]
    fn holding_the_breath_drowns_every_twenty_ticks_after_the_air_runs_out() {
        let mut breath = state(1, true);
        assert!(!breath.tick(&RULES, Breath::Hold));
        assert_eq!((breath.air, breath.breathing), (0, false));
        for air in (-19..=-1).rev() {
            assert!(!breath.tick(&RULES, Breath::Hold));
            assert_eq!(breath.air, air);
        }
        assert!(breath.tick(&RULES, Breath::Hold));
        assert_eq!(breath.air, 0);
    }

    #[test]
    fn breathing_refills_by_four_and_resets_negative_air_first() {
        let mut breath = state(-9, false);
        assert!(!breath.tick(&RULES, Breath::Breathe));
        assert_eq!((breath.air, breath.breathing), (0, true));
        breath.tick(&RULES, Breath::Breathe);
        assert_eq!(breath.air, 4);
        let mut breath = state(297, true);
        breath.tick(&RULES, Breath::Breathe);
        assert_eq!(breath.air, 300);
        breath.tick(&RULES, Breath::Breathe);
        assert_eq!(breath.air, 300);
    }

    #[test]
    fn respiration_spares_the_air_but_not_the_flag() {
        let mut breath = state(120, true);
        assert!(!breath.tick(&RULES, Breath::Spared));
        assert_eq!((breath.air, breath.breathing), (120, false));
    }

    #[test]
    fn a_bubble_column_fills_the_air_at_once() {
        let mut breath = state(-12, false);
        assert!(!breath.tick(&RULES, Breath::Bubbles));
        assert_eq!((breath.air, breath.breathing), (300, true));
    }

    #[test]
    fn restored_air_stays_within_the_supply() {
        assert_eq!(PlayerBreath::restored(&RULES, 39).air, 39);
        assert_eq!(PlayerBreath::restored(&RULES, -1).air, -1);
        assert_eq!(PlayerBreath::restored(&RULES, -40).air, -19);
        assert_eq!(PlayerBreath::restored(&RULES, 900).air, 300);
    }
}
