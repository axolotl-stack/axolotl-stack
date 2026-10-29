//! The damage the world deals players: landing after a fall, the player
//! definition's `minecraft:hurt_on_condition` (lava), fire blocks, burning
//! and the void.
//!
//! The client moves the player, so its fall is tracked from
//! `PlayerAuthInput` ([`PlayerFall::after_move`], called by the movement
//! system with the reported position and vertical collision). A landing
//! then goes through the mobs' landing rule
//! (`mob::combat::fall::resolve_landing`: landing-block multipliers, jump
//! boost, the 3-block safe distance) and hurts the player with
//! [`super::hurt::apply_hit`].
//!
//! Every tick each live player in survival or adventure is checked against
//! the blocks its collision box overlaps:
//! - water resets the fall distance and puts out [`Burning`];
//! - each hurt condition whose filter holds deals its damage per tick
//!   (under the hurt timer, so lava's 4 lands every 10 ticks);
//! - lava sets the player alight for [`LAVA_IGNITE_TICKS`] and fire blocks
//!   deal their damage and set it alight for [`FIRE_BLOCK_IGNITE_TICKS`];
//! - a burning player takes [`BURN_DAMAGE`] every [`BURN_INTERVAL_TICKS`];
//! - more than [`VOID_DEPTH`] below the world's minimum height (measured at
//!   the eyes, the player's position) the void deals [`VOID_DAMAGE`] per
//!   tick (creative players included) and resets the fall.

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use unastar_data::actor_components::body::ActorDamageCause;
use unastar_rules::filter::{BuiltinTest, Fact, FilterContext, FilterTest, Subject};

use super::constants::{
    BURN_DAMAGE, BURN_INTERVAL_TICKS, BURNING_BLOCKS, FIRE_BLOCK_IGNITE_TICKS, LAVA_IGNITE_TICKS,
    VOID_DAMAGE, VOID_DEPTH,
};
use super::hurt::{apply_hit, send_player_event};
use super::rules::PlayerRules;
use super::{Dead, hurt_player};
use crate::ecs::resources::TickCounter;
use crate::entity::PLAYER_EYE_HEIGHT;
use crate::entity::components::{FallDistance, GameMode, Player, Position};
use crate::entity::mob::combat::Hit;
use crate::entity::mob::combat::fall::{
    LandingTable, play_block_fall_sound, play_landing_sound, resolve_landing,
};
use crate::entity::mob::components::KnockbackPower;
use crate::entity::systems::BlockSource;
use crate::registry::BlockRegistry;
use crate::registry::block::LiquidKind;
use jolyne::valentine::types::EnumsActorEvent;

/// A player's fall, tracked from its movement input.
#[derive(Component, Debug, Clone, Copy, PartialEq, Default)]
pub struct PlayerFall {
    pub fall: FallDistance,
    /// The distance of a landing the server has not resolved yet (several
    /// inputs can arrive in one tick).
    pub landing: Option<f64>,
}

impl PlayerFall {
    /// Track one move of `dy` blocks that ended `on_ground`. A flying
    /// player does not fall.
    pub fn after_move(&mut self, dy: f64, on_ground: bool, flying: bool) {
        if flying {
            self.fall = FallDistance::default();
            return;
        }
        self.fall = self.fall.after_move(dy, on_ground, None);
        if let Some(distance) = self.fall.landed {
            self.landing = Some(
                self.landing
                    .map_or(distance, |earlier| earlier.max(distance)),
            );
        }
    }
}

/// A burning player: it burns until `until_tick` and takes its next burn
/// damage at `next_burn_tick`.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
#[component(storage = "SparseSet")]
pub struct Burning {
    pub until_tick: u64,
    pub next_burn_tick: u64,
}

/// What a block does to a player whose box overlaps it.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Contact {
    None,
    Water,
    Lava,
    /// A fire block and its damage per tick.
    Fire(f32),
}

/// [`Contact`] per block runtime ID.
pub struct ContactTable {
    contacts: Vec<Contact>,
}

impl ContactTable {
    pub fn new(registry: &BlockRegistry) -> Self {
        let mut contacts = Vec::new();
        for entry in registry.iter() {
            let fire = BURNING_BLOCKS
                .iter()
                .find(|(name, _)| *name == entry.string_id)
                .map(|(_, damage)| *damage);
            for id in entry.min_state_id..=entry.max_state_id {
                let index = id as usize;
                if contacts.len() <= index {
                    contacts.resize(index + 1, Contact::None);
                }
                contacts[index] = match (registry.liquid_state(id).map(|l| l.kind), fire) {
                    (Some(LiquidKind::Water), _) => Contact::Water,
                    (Some(LiquidKind::Lava), _) => Contact::Lava,
                    (None, Some(damage)) => Contact::Fire(damage),
                    (None, None) => Contact::None,
                };
            }
        }
        Self { contacts }
    }

    fn get(&self, runtime_id: u32) -> Contact {
        self.contacts
            .get(runtime_id as usize)
            .copied()
            .unwrap_or(Contact::None)
    }
}

/// The blocks a player's collision box overlaps.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
struct Overlap {
    water: bool,
    lava: bool,
    /// The strongest fire block's damage.
    fire: Option<f32>,
}

fn overlap(world: &World, table: &ContactTable, feet: DVec3, rules: &PlayerRules) -> Overlap {
    const INSET: f64 = 1.0e-3;
    let half = rules.width / 2.0 - INSET;
    let min = (feet + DVec3::new(-half, INSET, -half)).floor().as_ivec3();
    let max = (feet + DVec3::new(half, rules.height - INSET, half))
        .floor()
        .as_ivec3();
    let mut found = Overlap::default();
    for x in min.x..=max.x {
        for y in min.y..=max.y {
            for z in min.z..=max.z {
                match world
                    .block_at(IVec3::new(x, y, z))
                    .map_or(Contact::None, |id| table.get(id))
                {
                    Contact::None => {}
                    Contact::Water => found.water = true,
                    Contact::Lava => found.lava = true,
                    Contact::Fire(damage) => {
                        found.fire = Some(found.fire.map_or(damage, |other| other.max(damage)));
                    }
                }
            }
        }
    }
    found
}

/// Filter facts about a player for its definition's filters (the tests
/// `PlayerRules` accepts).
struct PlayerFacts {
    overlap: Overlap,
}

impl FilterContext for PlayerFacts {
    fn fact(&self, subject: Subject, test: &FilterTest) -> Option<Fact> {
        if subject != Subject::Self_ {
            return None;
        }
        Some(match test.builtin()? {
            BuiltinTest::InLava => Fact::Bool(self.overlap.lava),
            BuiltinTest::InWater => Fact::Bool(self.overlap.water),
            _ => return None,
        })
    }
}

fn environment_hit(damage: f32, cause: ActorDamageCause) -> Hit {
    Hit {
        damage,
        cause,
        attacker: None,
        by_player: false,
        knockback_from: None,
        knockback_power: KnockbackPower::DEFAULT,
    }
}

/// Whether `player` is alive in a game mode the world can hurt.
fn hurtable(world: &World, player: Entity) -> bool {
    world.get::<Dead>(player).is_none()
        && world
            .get::<GameMode>(player)
            .is_some_and(GameMode::allows_damage)
}

/// System (exclusive, after movement): resolve the landings the players'
/// movement reported.
pub fn land_players(
    world: &mut World,
    players: &mut QueryState<(Entity, &Position, &mut PlayerFall), With<Player>>,
    mut table: Local<Option<LandingTable>>,
    mut due: Local<Vec<(Entity, DVec3, f64)>>,
) {
    due.extend(
        players
            .iter_mut(world)
            .filter_map(|(player, position, mut fall)| {
                let distance = fall.landing.take()?;
                Some((player, position.0 - DVec3::Y * PLAYER_EYE_HEIGHT, distance))
            }),
    );
    if due.is_empty() {
        return;
    }
    let table = table.get_or_insert_with(|| LandingTable::new(world.registry()));
    for (player, feet, distance) in due.drain(..) {
        if !hurtable(world, player) {
            continue;
        }
        let Some(outcome) = resolve_landing(world, table, player, feet, distance) else {
            continue;
        };
        if outcome.dust {
            send_player_event(world, player, EnumsActorEvent::GroundDust);
        }
        if outcome.damage <= 0.0 {
            continue;
        }
        play_landing_sound(world, player, feet, outcome.damage);
        apply_hit(
            world,
            player,
            &environment_hit(outcome.damage as f32, outcome.cause),
            Some(distance),
        );
        play_block_fall_sound(world, player, feet, outcome.block);
    }
}

/// The players the environment can hurt: alive, with a position.
type LivingPlayers = QueryState<(Entity, &'static Position), (With<Player>, Without<Dead>)>;

/// System (exclusive): lava, fire, burning and the void.
pub fn hurt_players_by_environment(
    world: &mut World,
    players: &mut LivingPlayers,
    mut table: Local<Option<ContactTable>>,
    mut due: Local<Vec<(Entity, DVec3)>>,
) {
    due.extend(
        players
            .iter(world)
            .map(|(player, position)| (player, position.0)),
    );
    if due.is_empty() {
        return;
    }
    let table = table.get_or_insert_with(|| ContactTable::new(world.registry()));
    let rules = world.resource::<PlayerRules>().clone();
    let void = f64::from(crate::world::chunk::MIN_Y) - VOID_DEPTH;
    for (player, eyes) in due.drain(..) {
        let feet = eyes - DVec3::Y * PLAYER_EYE_HEIGHT;
        if eyes.y < void {
            if let Some(mut fall) = world.get_mut::<PlayerFall>(player) {
                fall.fall = FallDistance::default();
            }
            hurt_player(
                world,
                player,
                &environment_hit(VOID_DAMAGE, ActorDamageCause::Void),
            );
        }
        if !hurtable(world, player) {
            continue;
        }
        let overlap = overlap(world, table, feet, &rules);
        let now = world.resource::<TickCounter>().current;
        if overlap.water {
            if let Some(mut fall) = world.get_mut::<PlayerFall>(player) {
                fall.fall = FallDistance::default();
            }
            world.entity_mut(player).remove::<Burning>();
        }
        let facts = PlayerFacts { overlap };
        for condition in rules.hurt_conditions.iter() {
            if !hurtable(world, player) {
                break;
            }
            if condition.filter.evaluate(&facts) {
                hurt_player(
                    world,
                    player,
                    &environment_hit(
                        condition.damage_per_tick as f32,
                        condition.cause.unwrap_or(ActorDamageCause::None),
                    ),
                );
            }
        }
        if let Some(damage) = overlap.fire
            && hurtable(world, player)
        {
            hurt_player(
                world,
                player,
                &environment_hit(damage, ActorDamageCause::Fire),
            );
        }
        if !overlap.water {
            let ignite = if overlap.lava {
                Some(LAVA_IGNITE_TICKS)
            } else {
                overlap.fire.map(|_| FIRE_BLOCK_IGNITE_TICKS)
            };
            if let Some(ticks) = ignite
                && hurtable(world, player)
            {
                ignite_player(world, player, now, ticks);
            }
        }
        burn(world, player, now);
    }
}

/// Set `player` alight for at least `ticks` from `now`.
pub fn ignite_player(world: &mut World, player: Entity, now: u64, ticks: u64) {
    let burning = match world.get::<Burning>(player).copied() {
        Some(burning) => Burning {
            until_tick: burning.until_tick.max(now + ticks),
            ..burning
        },
        None => Burning {
            until_tick: now + ticks,
            next_burn_tick: now + BURN_INTERVAL_TICKS,
        },
    };
    world.entity_mut(player).insert(burning);
}

/// One tick of burning: the burn damage when it is due; the fire goes out
/// at its deadline.
fn burn(world: &mut World, player: Entity, now: u64) {
    let Some(burning) = world.get::<Burning>(player).copied() else {
        return;
    };
    if now >= burning.until_tick {
        world.entity_mut(player).remove::<Burning>();
        return;
    }
    if now < burning.next_burn_tick || !hurtable(world, player) {
        return;
    }
    world.entity_mut(player).insert(Burning {
        next_burn_tick: now + BURN_INTERVAL_TICKS,
        ..burning
    });
    hurt_player(
        world,
        player,
        &environment_hit(BURN_DAMAGE, ActorDamageCause::FireTick),
    );
}
