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
//!    zombie's 2.5 / 3 / 4.5). A hit scaled to nothing is ignored.
//! 3. The hurt timer, shared with mobs (`mob::combat::HurtTimer`): for
//!    [`HURT_TICKS`] after a full hit a new one only deals what exceeds the
//!    damage that started the timer, and nothing when it does not exceed it
//!    (override and self-destruct damage ignore the timer).
//! 4. Armour: the summed defence and toughness of the worn pieces reduce
//!    the damage of the causes armour stops (`unastar_data::armour`).
//!
//! A full hit sends `ActorEvent` hurt to the player and its viewers and,
//! when the hit has a source position, knocks the player back: the player
//! moves itself, so the push goes to its own client as `SetActorMotion`.
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
use super::{Dead, hooks, send_to_player_and_viewers};
use crate::ecs::resources::TickCounter;
use crate::entity::EntityRandom;
use crate::entity::components::{
    ArmourInventory, GameMode, Health, Player, PlayerSession, Position, RuntimeEntityId,
};
use crate::entity::mob::combat::{Hit, HurtTimer, knockback_velocity};
use crate::entity::mob::components::KnockbackPower;
use crate::entity::mob::constants::HURT_TICKS;
use crate::entity::mob::state::LastHurt;
use crate::world::rules::{Difficulty, GameRules};

/// Apply `hit` to `player`; returns the health actually removed (0 when the
/// hit was ignored).
pub fn hurt_player(world: &mut World, player: Entity, hit: &Hit) -> f32 {
    apply_hit(world, player, hit, None)
}

/// [`hurt_player`] for a landing after falling `fall_distance` blocks (the
/// death message tells short falls from long ones).
pub(super) fn apply_hit(
    world: &mut World,
    player: Entity,
    hit: &Hit,
    fall_distance: Option<f64>,
) -> f32 {
    apply_hit_result(world, player, hit, fall_distance).0
}

/// Apply one hit and retain its committed death outcome across immediate respawn.
pub(super) fn apply_hit_result(
    world: &mut World,
    player: Entity,
    hit: &Hit,
    fall_distance: Option<f64>,
) -> (f32, bool) {
    if !eligible_hit(world, player, hit) {
        return (0.0, false);
    }
    let mut event = hooks::PlayerDamage::new(player, *hit);
    hooks::damage(world, &mut event);
    if event.is_cancelled()
        || !eligible_hit(world, player, &event.hit)
        || !event.post_armour_multiplier.is_finite()
        || event.post_armour_multiplier < 0.0
    {
        return (0.0, false);
    }
    let hit = &event.hit;
    let damage = if hit.attacker.is_some() && !hit.by_player {
        let scaled = scale_for_difficulty(*world.resource::<Difficulty>(), hit.damage);
        if scaled <= 0.0 {
            return (0.0, false);
        }
        scaled
    } else {
        hit.damage
    };

    if !damage.is_finite() {
        return (0.0, false);
    }
    let ignores_timer = matches!(
        hit.cause,
        ActorDamageCause::Override | ActorDamageCause::SelfDestruct
    );
    let now = world.resource::<TickCounter>().current;
    let (dealt, until_tick, full) = match world.get::<HurtTimer>(player).copied() {
        Some(timer) if now < timer.until_tick && !ignores_timer => {
            if damage <= timer.last_damage {
                return (0.0, false);
            }
            (damage - timer.last_damage, timer.until_tick, false)
        }
        _ => (damage, now + u64::from(HURT_TICKS), true),
    };
    let dealt = if !event.bypass_armour && reduced_by_armour(hit.cause) {
        let (defence, toughness) = worn_armour(world, player);
        unastar_data::armour::damage_after_armour(dealt, defence, toughness)
    } else {
        dealt
    };
    let dealt = dealt * event.post_armour_multiplier;
    if !dealt.is_finite() {
        return (0.0, false);
    }

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
    if full {
        send_hurt_event(world, player);
        if let Some(from) = hit.knockback_from {
            knock_back(world, player, from, hit.knockback_power);
        }
    }
    // A hit that hurt wears the worn armour and costs the player food.
    if applied > 0.0 {
        crate::server::game::durability::wear_armour(world, player, damage, hit.cause);
        if crate::server::game::durability::armour_wears_from(hit.cause) {
            crate::server::game::hunger::exhaust_player(world, player, |values| values.damage);
        }
        hooks::damaged(
            world,
            hooks::PlayerDamaged {
                player,
                hit: *hit,
                damage: applied,
            },
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
    (applied, dead)
}

/// Validate an incoming or extension-adjusted hit against current player state.
/// This is repeated after callbacks, which may change or remove that state.
fn eligible_hit(world: &World, player: Entity, hit: &Hit) -> bool {
    if crate::entity::conditions::rejects_fire_damage(world, player, hit.cause) {
        return false;
    }
    let power = hit.knockback_power;
    if !hit.damage.is_finite()
        || hit.damage < 0.0
        || hit
            .knockback_from
            .is_some_and(|from| !from.is_finite() || !from.as_vec3().is_finite())
        || [
            power.horizontal,
            power.vertical,
            power.vertical_cap,
            power.retain,
        ]
        .into_iter()
        .any(|value| !value.is_finite() || !(value as f32).is_finite() || value < 0.0)
        || world.get::<Player>(player).is_none()
        || hit.knockback_from.is_some()
            && world
                .get::<Position>(player)
                .is_none_or(|position| !position.0.is_finite() || !position.0.as_vec3().is_finite())
        || world.get::<Dead>(player).is_some()
        || world.get::<Health>(player).is_none_or(|health| {
            !health.current.is_finite() || !health.max.is_finite() || health.is_dead()
        })
    {
        return false;
    }
    let mode = world.get::<GameMode>(player).copied().unwrap_or_default();
    if !mode.allows_damage()
        && !matches!(
            hit.cause,
            ActorDamageCause::Void | ActorDamageCause::Override
        )
    {
        return false;
    }
    world
        .get_resource::<GameRules>()
        .is_some_and(|rules| allowed_by_rules(rules, hit.cause))
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
    let Some(runtime_id) = world.get::<RuntimeEntityId>(player) else {
        return;
    };
    let packet = McpePacket::from(ActorEventPacket {
        target_runtime_id: ActorRuntimeId {
            actor_runtime_id: runtime_id.0 as u64,
        },
        event_id: event,
        data: 0,
        fire_at_position: None,
    });
    send_to_player_and_viewers(world, player, &packet);
}

fn send_hurt_event(world: &World, player: Entity) {
    send_player_event(world, player, EnumsActorEvent::Hurt);
}

/// Push `player` away from `from` (horizontally) with the native knockback
/// ([`knockback_velocity`]). The server does not track a player's own
/// motion, so the push starts from rest; it is sent to the player's client.
fn knock_back(world: &mut World, player: Entity, from: DVec3, power: KnockbackPower) {
    let Some(position) = world.get::<Position>(player).map(|p| p.0) else {
        return;
    };
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
        return;
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
