//! A mob's melee hit (`minecraft:attack` with `behavior.melee_box_attack`),
//! dealt after the AI tick that decided it.
//!
//! In native order: viewers see `ActorEvent` StartAttacking (twice when the
//! mob holds something); the damage is a uniform draw from the `attack`
//! range plus the held weapon's damage, raised by Strength and lowered by
//! Weakness; the hit (cause `entity_attack`, knocking the target away from
//! the mob) goes to [`hurt_player`] or [`hurt_mob`]. When it hurt, a burning
//! mob with `can_spread_on_fire` and an empty main hand may set the target
//! alight ([`MELEE_FIRE_SPREAD_CHANCE`]), `on_attack` fires, then `on_kill`
//! if the target died, and an `attack_once` goal is spent
//! ([`MeleeAttackedOnce`]). The `attack` effect is applied to a living
//! target that is not immune to the hit (creative and spectator players),
//! hurt or not.

use bevy_ecs::prelude::*;
use jolyne::valentine::types::EnumsActorEvent;
use rand::Rng;
use tracing::warn;
use unastar_data::actor_components::body::ActorDamageCause;

use super::{Hit, fire, hurt_mob, knockback_power_of, send_actor_event};
use crate::ecs::resources::TickCounter;
use crate::entity::EntityRandom;
use crate::entity::components::{EffectType, Effects, GameMode, Health, Player, Position};
use crate::entity::mob::MobKind;
use crate::entity::mob::components::{AttackConfig, MeleeBoxAttackGoal, SelfEvent};
use crate::entity::mob::constants::{
    MELEE_FIRE_SPREAD_CHANCE, STRENGTH_DAMAGE_ADD, STRENGTH_DAMAGE_SCALE, TICKS_PER_SECOND,
    WEAKNESS_DAMAGE_ADD, WEAKNESS_DAMAGE_SCALE,
};
use crate::entity::mob::events::fire_event;
use crate::entity::mob::gear;
use crate::entity::mob::state::{Dead, MeleeAttackedOnce};
use crate::entity::player_life::environment::ignite_player;
use crate::entity::player_life::hurt_player;
use crate::world::rules::Difficulty;

/// `attacker` hits `target` (the melee goal found it in reach this tick).
pub fn melee_attack(world: &mut World, attacker: Entity, target: Entity) {
    let (Some(attack), Some(goal), Some(feet)) = (
        world.get::<AttackConfig>(attacker).cloned(),
        world.get::<MeleeBoxAttackGoal>(attacker).cloned(),
        world.get::<Position>(attacker).map(|p| p.0),
    ) else {
        return;
    };
    if world.get::<Dead>(attacker).is_some()
        || !crate::entity::mob::ai::combat_candidate(world, attacker, target)
    {
        return;
    }
    let holding = gear::mainhand(world, attacker).is_some();
    send_actor_event(world, attacker, EnumsActorEvent::StartAttacking, 0);
    if holding {
        send_actor_event(world, attacker, EnumsActorEvent::StartAttacking, 0);
    }

    let damage = attack_damage(world, attacker, &attack);
    let hit = Hit {
        damage,
        cause: ActorDamageCause::EntityAttack,
        attacker: Some(attacker),
        by_player: false,
        knockback_from: Some(feet),
        knockback_power: knockback_power_of(world, attacker),
    };
    let is_player = world.get::<Player>(target).is_some();
    let dealt = if is_player {
        hurt_player(world, target, &hit)
    } else {
        hurt_mob(world, target, hit)
    };
    if dealt > 0.0 {
        after_hurt(world, attacker, target, &goal, holding, is_player);
    }
    if let Some(effect) = attack.effect
        && alive(world, target)
        && !immune(world, target)
    {
        world
            .entity_mut(target)
            .entry::<Effects>()
            .or_default()
            .get_mut()
            .add(effect.effect, 0, effect.duration_ticks);
    }
}

/// What a hit that hurt the target sets off.
fn after_hurt(
    world: &mut World,
    attacker: Entity,
    target: Entity,
    goal: &MeleeBoxAttackGoal,
    holding: bool,
    is_player: bool,
) {
    let difficulty = world
        .get_resource::<Difficulty>()
        .copied()
        .unwrap_or_default();
    let chance = MELEE_FIRE_SPREAD_CHANCE[difficulty as usize];
    if goal.can_spread_on_fire
        && fire::is_burning(world, attacker)
        && world.resource_mut::<EntityRandom>().0.r#gen::<f32>() < chance
        && !holding
    {
        let ticks = (chance as i32 * 2) as u64 * TICKS_PER_SECOND as u64;
        if is_player {
            let now = world.resource::<TickCounter>().current;
            if ticks > 0 {
                ignite_player(world, target, now, ticks);
            }
        } else {
            fire::ignite(world, target, ticks);
        }
    }
    if let Some(event) = &goal.on_attack {
        fire_self_event(world, attacker, event, target);
    }
    if !alive(world, target)
        && let Some(event) = &goal.on_kill
    {
        fire_self_event(world, attacker, event, target);
    }
    if goal.attack_once {
        world.entity_mut(attacker).insert(MeleeAttackedOnce);
    }
}

/// The native mob attack damage: the `attack` draw plus the held weapon,
/// then the attacker's Strength and Weakness levels.
fn attack_damage(world: &mut World, attacker: Entity, attack: &AttackConfig) -> f32 {
    let base = if attack.damage_max > attack.damage_min {
        world
            .resource_mut::<EntityRandom>()
            .0
            .gen_range(attack.damage_min..=attack.damage_max)
    } else {
        attack.damage_min
    };
    let mut damage = base + weapon_damage(world, attacker);
    if let Some(effects) = world.get::<Effects>(attacker) {
        if let Some(strength) = effects.get(EffectType::STRENGTH) {
            for _ in 0..=strength.level {
                damage = damage * STRENGTH_DAMAGE_SCALE + STRENGTH_DAMAGE_ADD;
            }
        }
        if let Some(weakness) = effects.get(EffectType::WEAKNESS) {
            for _ in 0..=weakness.level {
                damage = damage * WEAKNESS_DAMAGE_SCALE + WEAKNESS_DAMAGE_ADD;
                if damage < 0.0 {
                    return 0.0;
                }
            }
        }
    }
    damage
}

/// The held item's own attack damage: its measured player damage less the
/// bare hand's (the player's base attack damage, which a mob replaces with
/// its `attack` draw).
fn weapon_damage(world: &World, attacker: Entity) -> f32 {
    gear::mainhand(world, attacker).map_or(0.0, |stack| {
        unastar_data::melee::attack_damage(Some(stack.item_id()))
            - unastar_data::melee::attack_damage(None)
    })
}

fn alive(world: &World, entity: Entity) -> bool {
    world.get::<Dead>(entity).is_none() && world.get::<Health>(entity).is_some_and(|h| !h.is_dead())
}

/// A target the attack's damage source cannot hurt: a creative or spectator
/// player.
fn immune(world: &World, target: Entity) -> bool {
    world.get::<Player>(target).is_some()
        && !world
            .get::<GameMode>(target)
            .copied()
            .unwrap_or_default()
            .allows_damage()
}

fn fire_self_event(world: &mut World, mob: Entity, event: &SelfEvent, target: Entity) {
    let Some(handle) = world
        .get::<MobKind>(mob)
        .and_then(|kind| kind.0.events.event(&event.0))
    else {
        warn!(event = %event.0, "melee attack names an event the mob does not define");
        return;
    };
    if let Err(error) = fire_event(world, mob, &handle, Some(target)) {
        warn!(event = %event.0, %error, "melee attack event failed");
    }
}
