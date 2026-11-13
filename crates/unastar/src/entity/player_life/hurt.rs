//! Hurting players: [`hurt_player`] and the rules it applies, in native
//! order.
//!
//! 1. A dead player, or one whose game mode is immune (creative and
//!    spectator; void and override damage still hurt them), takes nothing.
//!    Neither does a cause its game rule turns off (`fall_damage`,
//!    `fire_damage`, `drowning_damage`, `freeze_damage`; read every hit, so
//!    `/gamerule` acts at once: BDS 1.26.51 refuses `/damage` of each of
//!    those causes while its rule is off).
//! 2. Damage from a non-player attacker scales with the difficulty:
//!    peaceful none, easy `min(damage / 2 + 1, damage)`, normal as is, hard
//!    × 1.5 (provisional: Minecraft Wiki "Difficulty", which gives the
//!    zombie's 2.5 / 3 / 4.5). A hit scaled to nothing is ignored.
//! 3. Armour: the summed defence and toughness of the worn pieces reduce
//!    the damage of the causes armour stops (`unastar_data::armour`); for
//!    melee, Protection (4 % a level) and Resistance (20 % a level) follow.
//! 4. The hurt timer, shared with mobs (`mob::combat::HurtTimer`): for
//!    [`HURT_TICKS`] after a hit a new one only deals what exceeds the
//!    damage that started the timer, both after step 3, and nothing when it
//!    does not exceed it; a hit that exceeds it restarts the timer (override
//!    and self-destruct damage ignore the timer). A player's melee hit is
//!    counted at the tick its packet arrived in (BDS 1.26.51 PvP sweep).
//!
//! A hit that leaves the player alive, a hit that exceeds the hurt window
//! included, knocks it back when the hit has a source position: half the
//! player's velocity kept, scaled by its armour's knockback resistance, one
//! `SetActorMotion` per push to the player and its viewers; then
//! `ActorEvent` hurt, its data the damage cause's ID, to the player and its
//! viewers. A killing hit sends neither: the death event is all the
//! clients see of it (BDS 1.26.51: the drowning death of
//! `testdata/bds-1.26.51-breath.json` and the slime kill of
//! `testdata/bds-1.26.51-player-data.json`). A player's melee hit also sends
//! `SetLastHurtBy` first and its particles last, a killing one too
//! ([`super::pvp`]).
//! The health change reaches the client through
//! `server::game::attributes::sync_player_attributes`; at zero health the player dies
//! (`death`).
//!
//! [`HURT_TICKS`]: crate::entity::mob::constants::HURT_TICKS

use glam::{DVec2, DVec3};
use jolyne::valentine::types::{
    ActorRuntimeId, EnumsActorEvent, EnumsActorType, EnumsAnimatePacketPayloadAction, Vec3,
};
use jolyne::valentine::{
    ActorEventPacket, AnimatePacket, McpePacket, SetActorMotionPacket, SetLastHurtByPacket,
};
use rand::Rng;
use unastar_data::actor_components::body::ActorDamageCause;

use bevy_ecs::prelude::*;

use super::death::{self, DeathCause};
use super::{Dead, send_to_player_and_viewers};
use crate::ecs::resources::TickCounter;
use crate::entity::EntityRandom;
use crate::entity::components::{
    ArmourInventory, GameMode, Health, Player, PlayerInput, Position, RuntimeEntityId,
};
use crate::entity::mob::combat::{Hit, HurtTimer, knockback_velocity};
use crate::entity::mob::components::KnockbackPower;
use crate::entity::mob::constants::HURT_TICKS;
use crate::entity::mob::state::LastHurt;
use crate::world::rules::{Difficulty, GameRules};

/// Apply `hit` to `player`; returns the health actually removed (0 when the
/// hit was ignored).
pub fn hurt_player(world: &mut World, player: Entity, hit: &Hit) -> f32 {
    apply_player_hit(world, player, hit, PlayerHit::default())
}

/// [`hurt_player`] for a landing after falling `fall_distance` blocks (the
/// death message tells short falls from long ones).
pub(super) fn apply_hit(
    world: &mut World,
    player: Entity,
    hit: &Hit,
    fall_distance: Option<f64>,
) -> f32 {
    apply_player_hit(
        world,
        player,
        hit,
        PlayerHit {
            fall_distance,
            ..PlayerHit::default()
        },
    )
}

/// What a hit carries beyond [`Hit`].
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PlayerHit {
    /// A second push after the native one (a player's sprint or Knockback
    /// enchantment, `super::pvp`).
    pub extra_knockback: Option<KnockbackPower>,
    /// A player's critical hit.
    pub critical: bool,
    /// A player's hit with a weapon whose enchantment adds damage.
    pub enchanted: bool,
    /// The fall a landing ends (death messages).
    pub fall_distance: Option<f64>,
    /// A player's melee hit: the victim and its viewers get
    /// `SetLastHurtBy` (player) before the push.
    pub by_player_hit: bool,
    /// The tick the hurt window counts this hit at (the tick its packet
    /// arrived in); the current tick when `None`.
    pub at_tick: Option<u64>,
}

/// [`hurt_player`] with the extras of `extras`.
pub(super) fn apply_player_hit(
    world: &mut World,
    player: Entity,
    hit: &Hit,
    extras: PlayerHit,
) -> f32 {
    let fall_distance = extras.fall_distance;
    if hit.damage < 0.0
        || world.get::<Player>(player).is_none()
        || world.get::<Dead>(player).is_some()
    {
        return 0.0;
    }
    if world.get::<Health>(player).is_none_or(Health::is_dead) {
        return 0.0;
    }
    let mode = world.get::<GameMode>(player).copied().unwrap_or_default();
    if !mode.allows_damage()
        && !matches!(
            hit.cause,
            ActorDamageCause::Void | ActorDamageCause::Override
        )
    {
        return 0.0;
    }
    if !allowed_by_rules(world.resource::<GameRules>(), hit.cause) {
        return 0.0;
    }
    let damage = if hit.attacker.is_some() && !hit.by_player {
        let scaled = scale_for_difficulty(*world.resource::<Difficulty>(), hit.damage);
        if scaled <= 0.0 {
            return 0.0;
        }
        scaled
    } else {
        hit.damage
    };

    let ignores_timer = matches!(
        hit.cause,
        ActorDamageCause::Override | ActorDamageCause::SelfDestruct
    );
    // Armour wears from the hit before armour reduces it.
    let unreduced = damage;
    // Armour, Protection and Resistance reduce the hit first; the hurt
    // window then compares and subtracts what is left (BDS 1.26.51 PvP
    // sweep: against four diamond pieces 4 leaves 0.96 and 7 leaves 1.89,
    // and the 7 two ticks after the 4 deals 0.93).
    let damage = if reduced_by_armour(hit.cause) {
        let (defence, toughness) = worn_armour(world, player);
        unastar_data::armour::damage_after_armour(damage, defence, toughness)
    } else {
        damage
    };
    let damage = if hit.cause == ActorDamageCause::EntityAttack {
        after_resistance(world, player, after_protection(world, player, damage))
    } else {
        damage
    };
    let now = world.resource::<TickCounter>().current;
    let at = extras.at_tick.unwrap_or(now);
    // A hit that exceeds the window restarts it (BDS 1.26.51 PvP sweep: 4,
    // then 7 two ticks later, then 8 nine ticks after the 7 deals 1).
    let dealt = match world.get::<HurtTimer>(player).copied() {
        Some(timer) if at < timer.until_tick && !ignores_timer => {
            if damage <= timer.last_damage {
                return 0.0;
            }
            damage - timer.last_damage
        }
        _ => damage,
    };
    let until_tick = at + u64::from(HURT_TICKS);

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
    // A hit inside the hurt window that exceeds it knocks back and shows
    // the hurt like a full one (BDS 1.26.51 PvP sweep: 4 then 7 two ticks
    // later pushes twice and sends two hurt events; a burst of rising hits
    // pushes once per hit).
    // A player's hit tells the victim and its viewers who hurt it, a
    // killing one too (BDS 1.26.51 PvP sweep).
    if extras.by_player_hit {
        send_to_player_and_viewers(
            world,
            player,
            &McpePacket::from(SetLastHurtByPacket {
                last_hurt_by: EnumsActorType::Player,
            }),
        );
    }
    if !dead {
        if let Some(from) = hit.knockback_from {
            let pushes = [Some(hit.knockback_power), extras.extra_knockback];
            knock_back(world, player, from, pushes.iter().flatten().copied());
        }
        send_hurt_event(world, player, hit.cause);
    }
    {
        // A weapon whose enchantment adds damage, then a critical hit, show
        // their particles, a killing hit's too (BDS 1.26.51: Sharpness V and
        // a crit send both, in that order).
        for (shown, action, data) in [
            (
                extras.enchanted,
                EnumsAnimatePacketPayloadAction::Magiccriticalhit,
                15.0,
            ),
            (
                extras.critical,
                EnumsAnimatePacketPayloadAction::Criticalhit,
                55.0,
            ),
        ] {
            if let (true, Some(runtime_id)) = (shown, world.get::<RuntimeEntityId>(player)) {
                let packet = McpePacket::from(AnimatePacket {
                    action,
                    target_actor_runtime_id: ActorRuntimeId {
                        actor_runtime_id: runtime_id.0 as u64,
                    },
                    data,
                    swing_source: None,
                });
                send_to_player_and_viewers(world, player, &packet);
            }
        }
    }
    // A hit that hurt wears the worn armour and costs the player food.
    if applied > 0.0 {
        crate::server::game::durability::wear_armour(world, player, unreduced, hit.cause);
        if crate::server::game::durability::armour_wears_from(hit.cause) {
            crate::server::game::hunger::exhaust_player(world, player, |values| values.damage);
        }
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
    applied
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
        ActorDamageCause::Freezing => rules.freeze_damage,
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

/// `damage` after the Protection levels of the worn pieces: 4 % a level
/// (BDS 1.26.51 PvP sweep: Protection I on four diamond pieces 2.24 ->
/// 1.8816, IV 2.24 -> 0.8064; melee is the only cause measured).
fn after_protection(world: &World, player: Entity, damage: f32) -> f32 {
    let levels: u32 = world.get::<ArmourInventory>(player).map_or(0, |armour| {
        armour
            .slots()
            .iter()
            .filter(|stack| !stack.is_empty())
            .map(|stack| {
                u32::from(stack.enchantment_level(unastar_rules::ids::Enchantment::Protection))
            })
            .sum()
    });
    damage * (1.0 - (PROTECTION_PER_LEVEL * levels as f32).min(1.0))
}

/// Protection's share of the damage per level.
const PROTECTION_PER_LEVEL: f32 = 0.04;
/// Resistance's share of the damage per level (level V and up: none left).
const RESISTANCE_PER_LEVEL: f32 = 0.2;

/// `damage` under the Resistance effect: 20 % a level, none from level V
/// (BDS 1.26.51 PvP sweep: a diamond sword's 8 -> 6.4, 4.8, 3.2, 1.6, 0).
fn after_resistance(world: &World, player: Entity, damage: f32) -> f32 {
    let Some(level) = world
        .get::<crate::entity::components::Effects>(player)
        .and_then(|effects| effects.get(crate::entity::components::EffectType::RESISTANCE))
        .map(|effect| u32::from(effect.level) + 1)
    else {
        return damage;
    };
    damage * (1.0 - (RESISTANCE_PER_LEVEL * level as f32).min(1.0))
}

/// Summed knockback resistance of the pieces `player` wears (netherite: 0.1
/// each).
fn worn_knockback_resistance(world: &World, player: Entity) -> f64 {
    world.get::<ArmourInventory>(player).map_or(0.0, |armour| {
        armour
            .slots()
            .iter()
            .filter(|stack| !stack.is_empty())
            .filter_map(|stack| unastar_data::armour::armour_values(stack.item_id()))
            .map(|values| f64::from(values.knockback_resistance))
            .sum()
    })
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
    send_player_event_with_data(world, player, event, 0, None);
}

fn send_player_event_with_data(
    world: &World,
    player: Entity,
    event: EnumsActorEvent,
    data: i32,
    at: Option<Vec3>,
) {
    let Some(runtime_id) = world.get::<RuntimeEntityId>(player) else {
        return;
    };
    let packet = McpePacket::from(ActorEventPacket {
        target_runtime_id: ActorRuntimeId {
            actor_runtime_id: runtime_id.0 as u64,
        },
        event_id: event,
        data,
        fire_at_position: at,
    });
    send_to_player_and_viewers(world, player, &packet);
}

/// The hurt event: its data is the cause's ID; a fall's also carries the
/// player's position where it landed (BDS 1.26.51, every fall hurt of
/// `testdata/bds-1.26.51-movement.json`; the other causes carry none).
fn send_hurt_event(world: &World, player: Entity, cause: ActorDamageCause) {
    let at = (cause == ActorDamageCause::Fall)
        .then(|| world.get::<Position>(player))
        .flatten()
        .map(|position| Vec3 {
            x: position.0.x as f32,
            y: position.0.y as f32,
            z: position.0.z as f32,
        });
    send_player_event_with_data(world, player, EnumsActorEvent::Hurt, cause_id(cause), at);
}

/// The wire ID of a damage cause: its place in the published
/// `ActorDamageCause` enum, counting `override` as 0 (`none` is -1). BDS
/// 1.26.51 sends it as the hurt event's data (fall 5, lava 8, entity attack
/// 2, magic 14, drowning 9 and void 12 in `testdata/bds-1.26.51-breath.json`).
fn cause_id(cause: ActorDamageCause) -> i32 {
    // The generated enum declares the causes in the published order.
    cause as i32 - 1
}

/// Push `player` away from `from` (horizontally) with each of `pushes` in
/// turn, the native knockback ([`knockback_velocity`]) scaled by the
/// knockback resistance of its armour. The push keeps half the player's velocity: the one its last
/// input reported, which is what BDS simulates for it (at rest 0.0784
/// down, so a netherite-clad victim leaves at 0.6 x 0.48 - 0.0392), or an
/// earlier push of this tick. It goes to the player and its viewers as
/// `SetActorMotion` and counts as the player's next move (`super::motion`).
fn knock_back(
    world: &mut World,
    player: Entity,
    from: DVec3,
    pushes: impl Iterator<Item = KnockbackPower>,
) {
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
    let resistance = worn_knockback_resistance(world, player);
    let mut velocity = world
        .get::<PlayerInput>(player)
        .map_or(DVec3::ZERO, |input| input.velocity);
    let mut count = 0;
    for power in pushes {
        let power = if resistance > 0.0 {
            crate::entity::mob::combat::knockback::KnockbackResistance(resistance).resist(power)
        } else {
            power
        };
        velocity = knockback_velocity(velocity, away.normalize(), power);
        count += 1;
    }
    if let Some(mut input) = world.get_mut::<PlayerInput>(player) {
        input.velocity = velocity;
    }
    if let Some(mut moves) = world.get_mut::<super::motion::PlayerMoves>(player) {
        moves.push(velocity);
    }
    let Some(runtime_id) = world.get::<RuntimeEntityId>(player) else {
        return;
    };
    let packet = McpePacket::from(SetActorMotionPacket {
        target_runtime_id: ActorRuntimeId {
            actor_runtime_id: runtime_id.0 as u64,
        },
        motion: Vec3 {
            x: velocity.x as f32,
            y: velocity.y as f32,
            z: velocity.z as f32,
        },
        tick: crate::entity::components::input_tick_of(world, player),
    });
    // One `SetActorMotion` per push, each with the velocity after all of
    // them (BDS 1.26.51 PvP sweep: a sprint hit sends (0.6, 0.32) twice).
    for _ in 0..count {
        send_to_player_and_viewers(world, player, &packet);
    }
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
