//! Hurting players: [`hurt_player`] and the rules it applies, in native
//! order.
//!
//! 1. A dead player, or one whose game mode is immune (creative and
//!    spectator; void and override damage still hurt them), takes nothing.
//!    Neither does a cause its game rule turns off (`fall_damage`,
//!    `fire_damage`, `drowning_damage`).
//! 2. Damage from a non-player attacker scales with the difficulty:
//!    peaceful none, easy `min(damage / 2 + 1, damage)`, normal as is, hard
//!    × 1.5 (provisional: Minecraft Wiki "Difficulty", which gives the
//!    zombie's 2.5 / 3 / 4.5). Projectile damage is not scaled (matches BDS
//!    1.26.51: a skeleton's arrow deals the same on every difficulty). A
//!    hit scaled to nothing is ignored.
//! 3. The hurt timer, shared with mobs (`mob::combat::HurtTimer`): for
//!    [`HURT_TICKS`] after a full hit a new one only deals what exceeds the
//!    damage that started the timer, and nothing when it does not exceed it
//!    (override and self-destruct damage ignore the timer).
//! 4. Armour: the summed defence and toughness of the worn pieces reduce
//!    the damage of the causes armour stops (`unastar_data::armour`).
//!
//! A full hit with a source position first knocks the player back (the
//! player moves itself, so the push goes to its own client as
//! `SetActorMotion`), then sends `ActorEvent` hurt, its data the damage
//! cause id, to the player and its viewers.
//! The health change reaches the client through
//! `server::game::attributes::sync_player_attributes`; at zero health the player dies
//! (`death`).
//!
//! [`HURT_TICKS`]: crate::entity::mob::constants::HURT_TICKS

use glam::{DVec2, DVec3};
use jolyne::valentine::types::{ActorRuntimeId, EnumsActorEvent, Vec3};
use jolyne::valentine::{ActorEventPacket, McpePacket, SetActorMotionPacket};
use rand::Rng;
use unastar_data::actor_components::body::ActorDamageCause;

use bevy_ecs::prelude::*;

use super::death::{self, DeathCause};
use super::{Dead, send_to_player_and_viewers};
use crate::ecs::resources::TickCounter;
use crate::entity::EntityRandom;
use crate::entity::components::{
    ArmourInventory, GameMode, Health, Player, PlayerSession, Position, RuntimeEntityId,
};
use crate::entity::mob::combat::{Hit, HurtTimer, knockback_velocity};
use crate::entity::mob::components::KnockbackPower;
use crate::entity::mob::constants::{HURT_TICKS, damage_cause_id};
use crate::entity::mob::state::LastHurt;
use crate::world::rules::{Difficulty, GameRules};

/// Apply `hit` to `player`; returns the health actually removed (0 when the
/// hit was ignored).
pub fn hurt_player(world: &mut World, player: Entity, hit: &Hit) -> f32 {
    apply_hit(world, player, hit, None).0
}

/// [`hurt_player`] that also returns the knockback velocity the hit sent
/// the player (`None` when it sent none), for callers that add their own
/// push to it (explosions).
pub fn hurt_player_knocked(world: &mut World, player: Entity, hit: &Hit) -> (f32, Option<DVec3>) {
    apply_hit(world, player, hit, None)
}

/// [`hurt_player`] for a landing after falling `fall_distance` blocks (the
/// death message tells short falls from long ones).
pub(super) fn apply_hit(
    world: &mut World,
    player: Entity,
    hit: &Hit,
    fall_distance: Option<f64>,
) -> (f32, Option<DVec3>) {
    if hit.damage < 0.0
        || world.get::<Player>(player).is_none()
        || world.get::<Dead>(player).is_some()
    {
        return (0.0, None);
    }
    if world.get::<Health>(player).is_none_or(Health::is_dead) {
        return (0.0, None);
    }
    let mode = world.get::<GameMode>(player).copied().unwrap_or_default();
    if !mode.allows_damage()
        && !matches!(
            hit.cause,
            ActorDamageCause::Void | ActorDamageCause::Override
        )
    {
        return (0.0, None);
    }
    if !allowed_by_rules(world.resource::<GameRules>(), hit.cause) {
        return (0.0, None);
    }
    let damage =
        if hit.attacker.is_some() && !hit.by_player && hit.cause != ActorDamageCause::Projectile {
            let scaled = scale_for_difficulty(*world.resource::<Difficulty>(), hit.damage);
            if scaled <= 0.0 {
                return (0.0, None);
            }
            scaled
        } else {
            hit.damage
        };

    let ignores_timer = matches!(
        hit.cause,
        ActorDamageCause::Override | ActorDamageCause::SelfDestruct
    );
    let now = world.resource::<TickCounter>().current;
    let (dealt, until_tick, full) = match world.get::<HurtTimer>(player).copied() {
        Some(timer) if now < timer.until_tick && !ignores_timer => {
            if damage <= timer.last_damage {
                return (0.0, None);
            }
            (damage - timer.last_damage, timer.until_tick, false)
        }
        _ => (damage, now + u64::from(HURT_TICKS), true),
    };
    let dealt = if reduced_by_armour(hit.cause) {
        let (defence, toughness) = worn_armour(world, player);
        unastar_data::armour::damage_after_armour(dealt, defence, toughness)
    } else {
        dealt
    };

    let (applied, dead) = {
        let mut entity = world.entity_mut(player);
        entity.insert((
            HurtTimer {
                until_tick,
                last_damage: damage,
            },
            LastHurt {
                tick: now,
                cause: hit.cause,
                attacker: hit.attacker,
                by_player: hit.by_player,
            },
        ));
        let mut health = entity.get_mut::<Health>().expect("health checked above");
        let before = health.current;
        health.damage(dealt);
        (before - health.current, health.is_dead())
    };
    // The knockback reaches the client before the hurt event, as BDS sends
    // them.
    let mut knockback = None;
    if full {
        if let Some(from) = hit.knockback_from {
            knockback = knock_back(world, player, from, hit.knockback_power);
        }
        send_player_event_data(
            world,
            player,
            EnumsActorEvent::Hurt,
            damage_cause_id(hit.cause),
        );
    }
    if dead {
        death::die(
            world,
            player,
            DeathCause {
                cause: hit.cause,
                attacker: hit.attacker,
                fall_distance,
            },
        );
    }
    (applied, knockback)
}

/// Whether the game rules let `cause` hurt players.
fn allowed_by_rules(rules: &GameRules, cause: ActorDamageCause) -> bool {
    match cause {
        ActorDamageCause::Fall => rules.fall_damage,
        ActorDamageCause::Fire
        | ActorDamageCause::FireTick
        | ActorDamageCause::Lava
        | ActorDamageCause::Magma
        | ActorDamageCause::Campfire
        | ActorDamageCause::SoulCampfire => rules.fire_damage,
        ActorDamageCause::Drowning => rules.drowning_damage,
        _ => true,
    }
}

/// Mob damage under `difficulty`.
pub fn scale_for_difficulty(difficulty: Difficulty, damage: f32) -> f32 {
    match difficulty {
        Difficulty::Peaceful => 0.0,
        Difficulty::Easy => (damage / 2.0 + 1.0).min(damage),
        Difficulty::Normal => damage,
        Difficulty::Hard => damage * 1.5,
    }
}

/// Causes worn armour reduces (provisional: the Java Edition damage types
/// outside the `bypasses_armor` tag; the BDS armour sweep measured
/// `entity_attack`).
fn reduced_by_armour(cause: ActorDamageCause) -> bool {
    matches!(
        cause,
        ActorDamageCause::Contact
            | ActorDamageCause::EntityAttack
            | ActorDamageCause::Projectile
            | ActorDamageCause::Fire
            | ActorDamageCause::Lava
            | ActorDamageCause::BlockExplosion
            | ActorDamageCause::EntityExplosion
            | ActorDamageCause::Anvil
            | ActorDamageCause::Thorns
            | ActorDamageCause::FallingBlock
            | ActorDamageCause::Piston
            | ActorDamageCause::Magma
            | ActorDamageCause::Fireworks
            | ActorDamageCause::Lightning
            | ActorDamageCause::RamAttack
            | ActorDamageCause::Campfire
            | ActorDamageCause::SoulCampfire
            | ActorDamageCause::MaceSmash
    )
}

/// Summed defence and toughness of the pieces `player` wears.
fn worn_armour(world: &World, player: Entity) -> (f32, f32) {
    world
        .get::<ArmourInventory>(player)
        .map_or((0.0, 0.0), |armour| {
            armour
                .slots()
                .iter()
                .filter(|stack| !stack.is_empty())
                .filter_map(|stack| unastar_data::armour::armour_values(stack.item_id()))
                .fold((0.0, 0.0), |(defence, toughness), values| {
                    (defence + values.defence, toughness + values.toughness)
                })
        })
}

/// Send `ActorEvent(event)` about `player` to it and its viewers.
pub(super) fn send_player_event(world: &World, player: Entity, event: EnumsActorEvent) {
    send_player_event_data(world, player, event, 0);
}

/// [`send_player_event`] with event data.
fn send_player_event_data(world: &World, player: Entity, event: EnumsActorEvent, data: i32) {
    let Some(runtime_id) = world.get::<RuntimeEntityId>(player) else {
        return;
    };
    let packet = McpePacket::from(ActorEventPacket {
        target_runtime_id: ActorRuntimeId {
            actor_runtime_id: runtime_id.0 as u64,
        },
        event_id: event,
        data,
        fire_at_position: None,
    });
    send_to_player_and_viewers(world, player, &packet);
}

/// Push `player` away from `from` (horizontally) with the native knockback
/// ([`knockback_velocity`]). The server does not track a player's own
/// motion, so the push starts from rest; it is sent to the player's client.
/// Returns the velocity sent.
fn knock_back(
    world: &mut World,
    player: Entity,
    from: DVec3,
    power: KnockbackPower,
) -> Option<DVec3> {
    let position = world.get::<Position>(player).map(|p| p.0)?;
    let mut away = DVec2::new(position.x - from.x, position.z - from.z);
    if away.length_squared() < 1.0e-8 {
        let angle = world
            .get_resource_or_init::<EntityRandom>()
            .0
            .gen_range(0.0..std::f64::consts::TAU);
        away = DVec2::from_angle(angle);
    }
    let velocity = knockback_velocity(DVec3::ZERO, away.normalize(), power);
    let (Some(session), Some(runtime_id)) = (
        world.get::<PlayerSession>(player),
        world.get::<RuntimeEntityId>(player),
    ) else {
        return Some(velocity);
    };
    let _ = session.send(McpePacket::from(SetActorMotionPacket {
        target_runtime_id: ActorRuntimeId {
            actor_runtime_id: runtime_id.0 as u64,
        },
        motion: Vec3 {
            x: velocity.x as f32,
            y: velocity.y as f32,
            z: velocity.z as f32,
        },
        tick: crate::entity::components::input_tick_of(world, player),
    }));
    Some(velocity)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn difficulty_scales_mob_damage() {
        assert_eq!(scale_for_difficulty(Difficulty::Peaceful, 3.0), 0.0);
        assert_eq!(scale_for_difficulty(Difficulty::Easy, 3.0), 2.5);
        assert_eq!(scale_for_difficulty(Difficulty::Easy, 1.0), 1.0);
        assert_eq!(scale_for_difficulty(Difficulty::Normal, 3.0), 3.0);
        assert_eq!(scale_for_difficulty(Difficulty::Hard, 3.0), 4.5);
    }
}
