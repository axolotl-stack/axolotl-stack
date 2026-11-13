//! Player-versus-player melee, as BDS 1.26.51 applies it
//! (`testdata/bds-1.26.51-pvp.json`, three lockstep clients).
//!
//! [`player_hits_player`] is one hit, handled with the world as it is at the
//! moment the hit is applied. BDS applies an attack when it consumes the
//! attacker's next movement input, before that input's move, so the caller
//! passes the attacker's state at that point as a [`Swing`], and the tick
//! the attack's packet arrived in (the victim's hurt window counts from
//! it). The current schedule applies every input before every attack of a
//! tick, so it hands over the state the attacker had before its first input
//! of the tick ([`InputStart`], recorded by the movement system) and the
//! current tick; an ordered packet path can call the same function when it
//! consumes the input (decision 0046).
//!
//! In order:
//!
//! 1. The hit is refused, with nothing sent, when the attacker is dead, the
//!    victim is gone, dead, in creative or spectator, the `pvp` game rule is
//!    off, the transaction names another slot or item than the held one, the
//!    position it claims is more than [`CLAIM_TOLERANCE`] from the
//!    attacker's, or the attacker's and the victim's positions are more than
//!    [`SURVIVAL_REACH`] ([`CREATIVE_REACH`] in creative) apart. Facing and
//!    walls are not checked (BDS checks facing only with strict entity
//!    interactions, which are off by default).
//! 2. Damage: the held item's melee damage plus Sharpness
//!    ([`crate::entity::mob::combat::melee_damage`]); Strength and Weakness
//!    as for mobs; then × 1.5 for a critical hit (falling, not on the
//!    ground, no Blindness, no Slow Falling; sprinting allowed). Difficulty
//!    does not scale it.
//! 3. The victim's side ([`super::hurt`]): armour with toughness,
//!    Protection, Resistance, then the hurt window; `SetLastHurtBy`; the
//!    native push away from the attacker and a second push of power
//!    `1.15 × Knockback level + 1` while sprinting; the hurt event; the
//!    particles of a Sharpness weapon and of a crit. The attack sound plays
//!    in the next tick ([`play_hit_sounds`]).

use bevy_ecs::prelude::*;
use glam::DVec3;
use jolyne::valentine::types::{ItemUseOnActorInventoryTransaction, Vec3};
use jolyne::valentine::{LevelSoundEventPacket, McpePacket};
use unastar_data::actor_components::body::ActorDamageCause;
use unastar_rules::ids::Enchantment;

use super::hurt::{self, PlayerHit};
use crate::ecs::resources::TickCounter;
use crate::entity::components::{
    EffectType, Effects, GameMode, Health, Player, PlayerInput, PlayerSession, PlayerState,
    Position, RuntimeEntityId,
};
use crate::entity::mob::combat::Hit;
use crate::entity::mob::components::KnockbackPower;
use crate::entity::mob::constants::{
    STRENGTH_DAMAGE_ADD, STRENGTH_DAMAGE_SCALE, WEAKNESS_DAMAGE_ADD, WEAKNESS_DAMAGE_SCALE,
};
use crate::entity::mob::events::held_item;
use crate::entity::mob::state::Dead;
use crate::world::rules::GameRules;

/// Farthest the position an attack claims may be from the attacker's
/// (BDS 1.26.51: off by 6.0 along x or y hits, by 6.1 is refused).
pub const CLAIM_TOLERANCE: f64 = 6.0;
/// Farthest a survival or adventure attacker may be from the victim, between
/// their positions (BDS 1.26.51: hits at 6.70 blocks, none at 6.72, on an
/// axis and on the diagonal, so not to the victim's box).
pub const SURVIVAL_REACH: f64 = 6.7;
/// [`SURVIVAL_REACH`] for a creative attacker (hits at 12.0, none at 12.05).
pub const CREATIVE_REACH: f64 = 12.0;
/// A critical hit's damage factor.
pub const CRITICAL_FACTOR: f32 = 1.5;
/// The second push's power per Knockback level.
pub const KNOCKBACK_ENCHANT_POWER: f64 = 1.15;
/// The second push's power of a sprinting attacker.
pub const SPRINT_KNOCKBACK_POWER: f64 = 1.0;
/// The second push's lift per unit of power (before the native 1.2 scale).
pub const EXTRA_KNOCKBACK_LIFT: f64 = 0.1;

/// The attacker as the hit sees it.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Swing {
    /// Its eyes (a player's `Position`).
    pub eye: DVec3,
    pub on_ground: bool,
    /// It has moved down since it left the ground (a fall distance above 0).
    pub falling: bool,
    pub sprinting: bool,
}

impl Swing {
    /// `player`'s state as the world holds it now.
    pub fn now(world: &World, player: Entity) -> Option<Self> {
        let input = world.get::<PlayerInput>(player);
        Some(Self {
            eye: world.get::<Position>(player)?.0,
            on_ground: input.is_some_and(|input| input.on_ground),
            falling: world
                .get::<super::environment::PlayerFall>(player)
                .is_some_and(|fall| fall.fall.distance > 0.0),
            sprinting: world
                .get::<PlayerState>(player)
                .is_some_and(|state| state.sprinting),
        })
    }

    /// The state an attack applied this tick sees: the one before the
    /// player's first input of the tick, if it sent one ([`InputStart`]),
    /// else the current one.
    pub fn for_attack(world: &World, player: Entity) -> Option<Self> {
        let now = world.resource::<TickCounter>().current;
        match world.get::<InputStart>(player) {
            Some(start) if start.tick == now => Some(start.swing),
            _ => Self::now(world, player),
        }
    }
}

/// A player's state before the first movement input it sent in `tick`.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct InputStart {
    pub tick: u64,
    pub swing: Swing,
}

impl Default for InputStart {
    /// No input recorded yet.
    fn default() -> Self {
        Self {
            tick: u64::MAX,
            swing: Swing::default(),
        }
    }
}

/// Why a hit was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    AttackerDead,
    AttackerSpectator,
    AttackerNotPlayer,
    /// No live player with that runtime id, or the attacker itself.
    NoVictim,
    /// Creative and spectator players cannot be hit.
    VictimImmune,
    PvpOff,
    /// The transaction names another hotbar slot or item than the held one.
    HeldItem,
    ClaimedTooFar,
    OutOfReach,
}

/// The player with this runtime id.
pub fn player_by_runtime_id(world: &mut World, runtime_id: u64) -> Option<Entity> {
    let mut query = world.query_filtered::<(Entity, &RuntimeEntityId), With<Player>>();
    query
        .iter(world)
        .find(|(_, id)| id.0 as u64 == runtime_id)
        .map(|(entity, _)| entity)
}

/// Resolves an attack BDS held until `attacker`'s next input: the target is
/// the player the transaction names, `pre_input` the attacker's state from
/// before that input (take it with [`Swing::now`] before the input is
/// applied: position, ground contact, fall and sprint), `arrived` the tick
/// the attack's packet arrived in. This is the call an ordered packet path
/// makes when it consumes the input; the current schedule makes it from the
/// attack queue with [`Swing::for_attack`]. Returns the health removed, or
/// why the attack was refused ([`Refusal::NoVictim`] when the runtime id
/// names no player).
pub fn resolve_held_attack(
    world: &mut World,
    attacker: Entity,
    transaction: &ItemUseOnActorInventoryTransaction,
    pre_input: Swing,
    arrived: u64,
) -> Result<f32, Refusal> {
    let victim = player_by_runtime_id(world, transaction.runtime_id.actor_runtime_id)
        .ok_or(Refusal::NoVictim)?;
    player_hits_player(world, attacker, victim, pre_input, transaction, arrived)
}

/// `attacker` hits `victim` as described in the module docs; `arrived` is
/// the server tick the attack's packet arrived in, which the victim's hurt
/// window counts from. Returns the health removed (0 for a hit inside the
/// hurt window that does not exceed it) or why it was refused.
pub fn player_hits_player(
    world: &mut World,
    attacker: Entity,
    victim: Entity,
    swing: Swing,
    transaction: &ItemUseOnActorInventoryTransaction,
    arrived: u64,
) -> Result<f32, Refusal> {
    check(world, attacker, victim, swing, transaction)?;
    let crit = critical(world, attacker, swing);
    let held = held_item(world, attacker).cloned();
    let sharpness = held
        .as_ref()
        .map_or(0, |stack| stack.enchantment_level(Enchantment::Sharpness));
    let knockback_level = held
        .as_ref()
        .map_or(0, |stack| stack.enchantment_level(Enchantment::Knockback));
    let mut damage = crate::entity::mob::combat::melee_damage(held.as_ref());
    damage = with_effects(world, attacker, damage);
    if crit {
        damage *= CRITICAL_FACTOR;
    }
    let power = KNOCKBACK_ENCHANT_POWER * f64::from(knockback_level)
        + if swing.sprinting {
            SPRINT_KNOCKBACK_POWER
        } else {
            0.0
        };
    let extra = (power > 0.0).then_some(KnockbackPower {
        horizontal: power,
        vertical: power * EXTRA_KNOCKBACK_LIFT,
        ..KnockbackPower::DEFAULT
    });
    let applied = hurt::apply_player_hit(
        world,
        victim,
        &Hit {
            damage,
            cause: ActorDamageCause::EntityAttack,
            attacker: Some(attacker),
            by_player: true,
            knockback_from: Some(swing.eye),
            knockback_power: KnockbackPower::DEFAULT,
        },
        PlayerHit {
            extra_knockback: extra,
            critical: crit,
            enchanted: sharpness > 0,
            by_player_hit: true,
            at_tick: Some(arrived),
            ..PlayerHit::default()
        },
    );
    if applied > 0.0 {
        queue_hit_sound(world, attacker, swing.eye, crit);
    }
    // The attacker pays food for the swing and, for a hit that landed,
    // weapon durability.
    crate::server::game::hunger::exhaust_player(world, attacker, |values| values.attack);
    if applied > 0.0
        && let Some(entry) = held.as_ref().and_then(|stack| stack.item())
    {
        let wear = crate::server::game::durability::attack_wear(entry);
        crate::server::game::durability::wear_held(world, attacker, wear);
    }
    Ok(applied)
}

/// The refusals of step 1.
fn check(
    world: &World,
    attacker: Entity,
    victim: Entity,
    swing: Swing,
    transaction: &ItemUseOnActorInventoryTransaction,
) -> Result<(), Refusal> {
    if world.get::<Player>(attacker).is_none() {
        return Err(Refusal::AttackerNotPlayer);
    }
    if !alive(world, attacker) {
        return Err(Refusal::AttackerDead);
    }
    // Not measured (a vanilla spectator never attacks); refused as the mob
    // path refuses it, the server checking what the client claims.
    if world.get::<GameMode>(attacker) == Some(&GameMode::Spectator) {
        return Err(Refusal::AttackerSpectator);
    }
    if attacker == victim || world.get::<Player>(victim).is_none() || !alive(world, victim) {
        return Err(Refusal::NoVictim);
    }
    if !world
        .get::<GameMode>(victim)
        .copied()
        .unwrap_or_default()
        .allows_damage()
    {
        return Err(Refusal::VictimImmune);
    }
    if !world.resource::<GameRules>().pvp {
        return Err(Refusal::PvpOff);
    }
    if !holds_claimed_item(world, attacker, transaction) {
        return Err(Refusal::HeldItem);
    }
    let claimed = DVec3::new(
        f64::from(transaction.from_position.x),
        f64::from(transaction.from_position.y),
        f64::from(transaction.from_position.z),
    );
    if !claimed.is_finite() || claimed.distance(swing.eye) > CLAIM_TOLERANCE {
        return Err(Refusal::ClaimedTooFar);
    }
    let mode = world.get::<GameMode>(attacker).copied().unwrap_or_default();
    let reach = if mode == GameMode::Creative {
        CREATIVE_REACH
    } else {
        SURVIVAL_REACH
    };
    let target = world
        .get::<Position>(victim)
        .map(|position| position.0)
        .ok_or(Refusal::NoVictim)?;
    if swing.eye.distance(target) > reach {
        return Err(Refusal::OutOfReach);
    }
    Ok(())
}

/// Whether the transaction names the attacker's held hotbar slot and the
/// item in it (BDS 1.26.51: an attack naming another slot's sword while the
/// held slot is empty does nothing).
fn holds_claimed_item(
    world: &World,
    attacker: Entity,
    transaction: &ItemUseOnActorInventoryTransaction,
) -> bool {
    let slot = world
        .get::<crate::entity::components::HeldSlot>(attacker)
        .map_or(0, |slot| slot.0);
    let held = held_item(world, attacker)
        .and_then(|stack| stack.item())
        .map_or(0, |entry| entry.network_id());
    transaction.slot == i32::from(slot) && i32::from(transaction.item.id) == held
}

/// The sound of a hit that hurt: `attack.critical` or `attack.strong` at the
/// attacker's middle, to the players that see the attacker but not to the
/// attacker, with the packets of the next tick (BDS 1.26.51).
fn queue_hit_sound(world: &mut World, attacker: Entity, eye: DVec3, critical: bool) {
    let at = eye - DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT + DVec3::Y * HIT_SOUND_HEIGHT;
    world
        .get_resource_or_init::<HitSounds>()
        .0
        .push((attacker, at, critical));
}

/// Height of a hit's sound above the attacker's feet.
const HIT_SOUND_HEIGHT: f64 = 0.9;

/// Hit sounds waiting for the next tick.
#[derive(Resource, Debug, Default)]
pub struct HitSounds(Vec<(Entity, DVec3, bool)>);

/// Plays the hit sounds of the last tick (called once a tick, before the
/// tick's attacks).
pub fn play_hit_sounds(world: &mut World) {
    let Some(mut sounds) = world.get_resource_mut::<HitSounds>() else {
        return;
    };
    let sounds = std::mem::take(&mut sounds.0);
    for (attacker, at, critical) in sounds {
        let Some(unique_id) = world.get::<RuntimeEntityId>(attacker).map(|id| id.0) else {
            continue;
        };
        let packet = McpePacket::from(LevelSoundEventPacket {
            sound_event: if critical {
                "attack.critical"
            } else {
                "attack.strong"
            }
            .into(),
            position: Vec3 {
                x: at.x as f32,
                y: at.y as f32,
                z: at.z as f32,
            },
            data: -1,
            actor_identifier: "minecraft:player".into(),
            is_baby: false,
            is_global: false,
            actor_unique_id: unique_id,
            fire_at_position: None,
        });
        for &viewer in super::player_viewers(world, attacker) {
            if let Some(session) = world.get::<PlayerSession>(viewer) {
                let _ = session.send(packet.clone());
            }
        }
    }
}

fn alive(world: &World, player: Entity) -> bool {
    world.get::<Dead>(player).is_none() && world.get::<Health>(player).is_some_and(|h| !h.is_dead())
}

/// A critical hit: falling, off the ground, without Blindness or Slow
/// Falling.
fn critical(world: &World, attacker: Entity, swing: Swing) -> bool {
    let effects = world.get::<Effects>(attacker);
    let has = |effect| effects.is_some_and(|effects| effects.has(effect));
    swing.falling
        && !swing.on_ground
        && !has(EffectType::BLINDNESS)
        && !has(EffectType::SLOW_FALLING)
}

/// `damage` raised by each level of Strength, then lowered by each level of
/// Weakness (never below 0).
fn with_effects(world: &World, attacker: Entity, mut damage: f32) -> f32 {
    let Some(effects) = world.get::<Effects>(attacker) else {
        return damage;
    };
    if let Some(strength) = effects.get(EffectType::STRENGTH) {
        for _ in 0..=strength.level {
            damage = damage * STRENGTH_DAMAGE_SCALE + STRENGTH_DAMAGE_ADD;
        }
    }
    if let Some(weakness) = effects.get(EffectType::WEAKNESS) {
        for _ in 0..=weakness.level {
            damage = (damage * WEAKNESS_DAMAGE_SCALE + WEAKNESS_DAMAGE_ADD).max(0.0);
        }
    }
    damage
}
