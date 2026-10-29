//! Effect systems for status effects.

use bevy_ecs::prelude::*;
use jolyne::valentine::types::{ActorRuntimeId, EnumsMobEffectPacketPayloadEvent, PlayerInputTick};
use jolyne::valentine::{McpePacket, MobEffectPacket};
use unastar_data::actor_components::body::ActorDamageCause;

use crate::entity::components::*;
use crate::entity::mob::MobKind;
use crate::entity::mob::combat::{Hit, hurt_mob};
use crate::entity::mob::components::KnockbackPower;
use crate::entity::mob::constants::{POISON_DAMAGE, POISON_INTERVAL_TICKS, POISON_MIN_HEALTH};
use crate::entity::player_life::hurt_player;

/// System: Tick all status effects. A player whose effect runs out is told
/// so (`MobEffect` remove).
pub fn tick_effects(
    mut query: Query<(&mut Effects, Option<&PlayerSession>, Option<&RuntimeEntityId>)>,
) {
    for (mut effects, session, runtime_id) in query.iter_mut() {
        let expired = effects.tick();
        if let (Some(session), Some(runtime_id)) = (session, runtime_id) {
            for effect in expired {
                let _ = session.send(effect_packet(
                    runtime_id.0 as u64,
                    EnumsMobEffectPacketPayloadEvent::Remove,
                    effect,
                    &ActiveEffect::new(0, 0),
                ));
            }
        }
    }
}

/// The `MobEffect` packet telling a player's client about `effect` on the
/// actor with `runtime_id`.
pub fn effect_packet(
    runtime_id: u64,
    event: EnumsMobEffectPacketPayloadEvent,
    effect: EffectType,
    instance: &ActiveEffect,
) -> McpePacket {
    McpePacket::from(MobEffectPacket {
        target_runtime_id: ActorRuntimeId {
            actor_runtime_id: runtime_id,
        },
        event_id: event,
        effect_id: i32::from(effect.0),
        effect_amplifier: i32::from(instance.level),
        show_particles: instance.show_particles,
        effect_duration_ticks: instance.duration_ticks as i32,
        tick: PlayerInputTick { inputtick: 0 },
        ambient: instance.ambient,
    })
}

/// System: Apply regeneration effect healing.
pub fn apply_regeneration(mut query: Query<(&mut Health, &Effects), With<Living>>) {
    for (mut health, effects) in query.iter_mut() {
        if let Some(regen) = effects.get(EffectType::REGENERATION) {
            // Regeneration ticks every 50 / (level + 1) ticks
            // Level 1 = every 25 ticks, Level 2 = every 12.5 ticks, etc.
            let interval = 50 / (regen.level as u32 + 1);
            if regen.duration_ticks % interval == 0 {
                health.heal(1.0);
            }
        }
    }
}

/// System (exclusive): poison. While a player's or mob's poison runs, on
/// every tick whose remaining duration is a multiple of
/// `POISON_INTERVAL_TICKS >> level` (every tick once that is 0) it takes
/// [`POISON_DAMAGE`] magic damage through the hurt rules, but only while its
/// health is above [`POISON_MIN_HEALTH`]: poison never kills.
#[allow(clippy::type_complexity)]
pub fn apply_poison(
    world: &mut World,
    actors: &mut QueryState<
        (Entity, &Effects, &Health, Has<Player>),
        Or<(With<Player>, With<MobKind>)>,
    >,
    mut due: Local<Vec<(Entity, bool)>>,
) {
    due.extend(
        actors
            .iter(world)
            .filter(|(_, effects, health, _)| {
                effects.get(EffectType::POISON).is_some_and(|poison| {
                    let interval = POISON_INTERVAL_TICKS.checked_shr(u32::from(poison.level));
                    let interval = interval.unwrap_or(0);
                    health.current - POISON_DAMAGE >= POISON_MIN_HEALTH
                        && (interval == 0 || poison.duration_ticks % interval == 0)
                })
            })
            .map(|(entity, _, _, player)| (entity, player)),
    );
    for (entity, player) in due.drain(..) {
        let hit = Hit {
            damage: POISON_DAMAGE,
            cause: ActorDamageCause::Magic,
            attacker: None,
            by_player: false,
            knockback_from: None,
            knockback_power: KnockbackPower::DEFAULT,
        };
        if player {
            hurt_player(world, entity, &hit);
        } else {
            hurt_mob(world, entity, hit);
        }
    }
}

/// System: Apply wither effect damage.
pub fn apply_wither(mut query: Query<(&mut Health, &Effects), With<Living>>) {
    for (mut health, effects) in query.iter_mut() {
        if let Some(wither) = effects.get(EffectType::WITHER) {
            // Wither ticks every 40 / (level + 1) ticks and can kill
            let interval = 40 / (wither.level as u32 + 1);
            if wither.duration_ticks % interval == 0 {
                health.damage(1.0);
            }
        }
    }
}

/// System: Tick air supply for entities underwater.
pub fn tick_air_supply(mut query: Query<(&Position, &mut AirSupply, &mut Health), With<Living>>) {
    // TODO: Check if head is in water block
    // For now, assume not underwater
    for (_position, mut air_supply, mut _health) in query.iter_mut() {
        let is_underwater = false; // TODO: Block check
        let drowning = air_supply.tick(is_underwater);
        if drowning {
            // TODO: Apply drowning damage
        }
    }
}

/// Living entities other than mobs (mobs burn through `mob::combat::fire`).
type NonMobLiving = (With<Living>, Without<crate::entity::mob::MobKind>);

/// System: Tick fire duration.
pub fn tick_fire(mut query: Query<(&mut OnFire, &mut Health), NonMobLiving>) {
    for (mut on_fire, mut health) in query.iter_mut() {
        if on_fire.is_on_fire() {
            on_fire.tick();
            // Fire damage every 20 ticks
            if on_fire.ticks_remaining % 20 == 0 {
                health.damage(1.0);
            }
        }
    }
}

/// System: Tick damage immunity.
pub fn tick_damage_immunity(mut query: Query<&mut DamageImmunity>) {
    for mut immunity in query.iter_mut() {
        immunity.tick();
    }
}
