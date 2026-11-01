//! Kinetic weapon (spear) damage: a mob whose `behavior.use_kinetic_weapon`
//! goal raised its weapon carries [`DealKineticDamage`], built from the held
//! item's `minecraft:kinetic_weapon`, and hits whatever the weapon's tip
//! runs into.
//!
//! [`deal_kinetic_damage`] runs every tick after the physics step and the
//! mob AI, but measures every actor where it stood at the start of the tick,
//! before its move ([`TickStart`]). The weapon counts its ticks from the
//! raise (0 on that tick). Once the weapon's `delay` passed it traces the
//! segment from the mob's head along its look (head yaw and pitch), from
//! `reach.min` to `reach.max` blocks out. Every living player (not a
//! spectator) and mob other than the holder whose box, grown by half the
//! `hitbox_margin` on
//! every side, meets that segment before any block does is a candidate,
//! nearest first; an actor hit less than [`KINETIC_REHIT_TICKS`] ago is
//! skipped. The holder's speed is its move during the last tick along the
//! look (blocks per second, [`TickStart`]) and the relative speed that less
//! the target's. Each condition (`damage_conditions`, `knockback_conditions`)
//! holds while both speeds reach their minimums (a minimum of about zero is
//! not checked) and its `max_duration` (after the delay) has not run out.
//! A passing damage condition deals `floor(relative × damage_multiplier)`
//! (at least 0) plus the mob's `attack` damage, raised by Strength and
//! lowered by Weakness, without the held item's own damage; the hit knocks
//! the target back only when the knockback condition passes. A hit that hurt
//! shows `ActorEvent` KineticDamageDealt on the holder with the target's
//! unique id (low 32 bits) as data. Once every condition's `max_duration`
//! plus the delay ran out the component is removed (the goal then lowers
//! the weapon).
//!
//! Matches the BDS 1.26.51 recordings `zombie-spear-field` and
//! `zombie-door-reach-spear`. Riding is not implemented, so
//! `dismount_conditions` only count toward how long the weapon stays
//! raised.

use bevy_ecs::prelude::*;
use glam::DVec3;
use jolyne::valentine::types::EnumsActorEvent;
use unastar_data::actor_components::body::ActorDamageCause;
use unastar_data::item_components::{KineticConditions, KineticWeapon};

use super::melee::attack_damage_with;
use super::{Hit, hurt_mob, knockback_power_of, send_actor_event};
use crate::ecs::TickCounter;
use crate::entity::components::{GameMode, PhysicsBody, Player, Position, Rotation};
use crate::entity::mob::MobKind;
use crate::entity::mob::ai::Candidate;
use crate::entity::mob::ai::sensing::{first_block_hit, segment_entry};
use crate::entity::mob::components::AttackConfig;
use crate::entity::mob::constants::{
    KINETIC_HITBOX_MARGIN_SCALE, KINETIC_MIN_SPEED_EPSILON, KINETIC_REHIT_TICKS,
    MOB_HEAD_ATTACH_RATIO, TICKS_PER_SECOND,
};
use crate::entity::mob::net::network::actor_unique_id;
use crate::entity::mob::state::Dead;
use crate::entity::player_life::hurt_player;
use crate::registry::collision::Aabb;

/// One of a kinetic weapon's conditions, its minimum speeds scaled by the
/// goal's `weapon_min_speed_multiplier`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KineticCondition {
    /// Ticks after the delay the condition holds; negative: always.
    max_duration: i32,
    /// Blocks per second.
    min_speed: f32,
    /// Blocks per second.
    min_relative_speed: f32,
}

impl KineticCondition {
    fn new(conditions: &KineticConditions, min_speed_multiplier: f32) -> Self {
        Self {
            max_duration: conditions.max_duration,
            min_speed: conditions.min_speed.unwrap_or(0.0) * min_speed_multiplier,
            min_relative_speed: conditions.min_relative_speed.unwrap_or(0.0) * min_speed_multiplier,
        }
    }

    /// Whether the condition holds for a holder moving at `speed` and
    /// `relative` (blocks per second) `elapsed` ticks after the raise.
    fn passes(&self, speed: f32, relative: f32, elapsed: u64, delay: u64) -> bool {
        let short = |actual: f32, min: f32| min.abs() > KINETIC_MIN_SPEED_EPSILON && actual < min;
        if short(speed, self.min_speed) || short(relative, self.min_relative_speed) {
            return false;
        }
        match u64::try_from(self.max_duration) {
            Ok(duration) => elapsed < duration + delay,
            Err(_) => true,
        }
    }
}

/// A held item's `minecraft:kinetic_weapon` as a mob's goal uses it: its
/// reach scaled by `weapon_reach_multiplier`, its minimum speeds by
/// `weapon_min_speed_multiplier`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KineticSettings {
    /// Ticks after the raise before anything is hit.
    delay: u64,
    /// Blocks from the head the weapon starts hitting.
    reach_min: f64,
    /// Blocks from the head the weapon stops hitting.
    reach_max: f64,
    hitbox_margin: f64,
    damage_multiplier: f32,
    damage: KineticCondition,
    knockback: KineticCondition,
    dismount: KineticCondition,
}

impl KineticSettings {
    pub fn new(weapon: &KineticWeapon, reach_multiplier: f32, min_speed_multiplier: f32) -> Self {
        Self {
            delay: u64::try_from(weapon.delay).unwrap_or(0),
            reach_min: f64::from(weapon.reach.min * reach_multiplier),
            reach_max: f64::from(weapon.reach.max * reach_multiplier),
            hitbox_margin: f64::from(weapon.hitbox_margin),
            damage_multiplier: weapon.damage_multiplier,
            damage: KineticCondition::new(&weapon.damage_conditions, min_speed_multiplier),
            knockback: KineticCondition::new(&weapon.knockback_conditions, min_speed_multiplier),
            dismount: KineticCondition::new(&weapon.dismount_conditions, min_speed_multiplier),
        }
    }

    /// The middle of the weapon's reach: a target closer than this sends
    /// the goal's mob back to charge again.
    pub fn mid_reach(&self) -> f64 {
        (self.reach_min + self.reach_max) * 0.5
    }

    /// Ticks after the raise at which every condition ran out; `None` when
    /// one of them never runs out.
    fn done_after(&self) -> Option<u64> {
        [self.damage, self.knockback, self.dismount]
            .iter()
            .map(|condition| u64::try_from(condition.max_duration).ok())
            .try_fold(0, |longest: u64, duration| Some(longest.max(duration?)))
            .map(|longest| longest + self.delay)
    }
}

/// A raised kinetic weapon: present from the tick the goal raised it until
/// its conditions ran out or the goal lowered it.
#[derive(Component, Debug, Clone)]
#[component(storage = "SparseSet")]
pub struct DealKineticDamage {
    settings: KineticSettings,
    /// The tick the weapon was raised; its tick count is 0 then.
    raised_at: u64,
    /// The tick each actor was last hit.
    hits: Vec<(Entity, u64)>,
}

impl DealKineticDamage {
    /// A weapon raised on tick `now`.
    pub fn new(settings: KineticSettings, now: u64) -> Self {
        Self {
            settings,
            raised_at: now,
            hits: Vec::new(),
        }
    }
}

/// Where a mob or player stood at the start of this tick, before the
/// physics step moved it, and how far it moved during the last tick.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct TickStart {
    pub position: DVec3,
    pub moved: DVec3,
}

/// System: record every mob's and player's [`TickStart`] before the physics
/// step; an actor seen for the first time starts at rest.
#[allow(clippy::type_complexity)]
pub fn record_tick_start(
    mut commands: Commands,
    mut tracked: Query<(&Position, &mut TickStart)>,
    new: Query<(Entity, &Position), (Without<TickStart>, Or<(With<MobKind>, With<Player>)>)>,
) {
    for (position, mut start) in &mut tracked {
        let moved = position.0 - start.position;
        *start = TickStart {
            position: position.0,
            moved,
        };
    }
    for (entity, position) in &new {
        commands.entity(entity).insert(TickStart {
            position: position.0,
            moved: DVec3::ZERO,
        });
    }
}

/// Reused buffers of [`deal_kinetic_damage`].
#[derive(Default)]
pub struct KineticScratch {
    holders: Vec<Entity>,
    actors: Vec<Entity>,
    /// `(distance along the look, actor)` of this holder's candidates.
    struck: Vec<(f64, Entity)>,
}

/// System: every raised kinetic weapon hits what its tip runs into, then is
/// removed once its conditions ran out.
#[allow(clippy::type_complexity)]
pub fn deal_kinetic_damage(
    world: &mut World,
    holders: &mut QueryState<Entity, (With<DealKineticDamage>, Without<Dead>)>,
    actors: &mut QueryState<Entity, (With<Position>, Or<(With<MobKind>, With<Player>)>)>,
    mut scratch: Local<KineticScratch>,
) {
    let scratch = &mut *scratch;
    scratch.holders.clear();
    scratch.holders.extend(holders.iter(world));
    if scratch.holders.is_empty() {
        return;
    }
    scratch.actors.clear();
    scratch.actors.extend(actors.iter(world));
    let now = world.resource::<TickCounter>().current;
    for &holder in &scratch.holders {
        let Some(raised) = world.get::<DealKineticDamage>(holder) else {
            continue;
        };
        let (settings, elapsed) = (raised.settings, now.saturating_sub(raised.raised_at));
        if elapsed >= settings.delay {
            strike(
                world,
                holder,
                &settings,
                elapsed,
                now,
                &scratch.actors,
                &mut scratch.struck,
            );
        }
        if settings.done_after().is_some_and(|done| elapsed >= done) {
            world.entity_mut(holder).remove::<DealKineticDamage>();
        }
    }
}

/// The holder's weapon hits every candidate in its way that it may hit now.
fn strike(
    world: &mut World,
    holder: Entity,
    settings: &KineticSettings,
    elapsed: u64,
    now: u64,
    actors: &[Entity],
    struck: &mut Vec<(f64, Entity)>,
) {
    let (Some(start), Some(rotation), Some(body), Some(attack)) = (
        world.get::<TickStart>(holder).copied(),
        world.get::<Rotation>(holder).copied(),
        world.get::<PhysicsBody>(holder),
        world.get::<AttackConfig>(holder).cloned(),
    ) else {
        return;
    };
    let feet = start.position;
    let head = feet + DVec3::Y * body.height * MOB_HEAD_ATTACH_RATIO;
    let look = look_direction(rotation.head_yaw, rotation.pitch);
    let near = head + look * settings.reach_min;
    let end = head + look * settings.reach_max;
    // Nothing behind the first block the weapon's line meets is hit.
    let block = first_block_hit(world, head, end).map_or(f64::INFINITY, |t| t * settings.reach_max);
    let margin = DVec3::splat(settings.hitbox_margin * KINETIC_HITBOX_MARGIN_SCALE);
    struck.clear();
    struck.extend(actors.iter().filter_map(|&actor| {
        if actor == holder || world.get::<GameMode>(actor) == Some(&GameMode::Spectator) {
            return None;
        }
        let target = Candidate::of(world, actor)?;
        // The target's box where it stood at the start of the tick.
        let shift = world
            .get::<TickStart>(actor)
            .map_or(DVec3::ZERO, |start| start.position - target.pos);
        let aabb = Aabb::new(target.aabb.min + shift, target.aabb.max + shift).inflate(margin);
        let entry = segment_entry(near, end - near, &aabb)?;
        let distance = settings.reach_min + entry * (settings.reach_max - settings.reach_min);
        (distance <= block).then_some((distance, actor))
    }));
    if struck.is_empty() {
        return;
    }
    struck.sort_by(|a, b| a.0.total_cmp(&b.0));
    let speed = (look.dot(start.moved) * f64::from(TICKS_PER_SECOND)) as f32;
    for &(_, target) in struck.iter() {
        let recently = world
            .get::<DealKineticDamage>(holder)
            .is_some_and(|raised| {
                raised.hits.iter().any(|&(hit, at)| {
                    hit == target && now.saturating_sub(at) <= KINETIC_REHIT_TICKS
                })
            });
        if recently {
            continue;
        }
        let target_delta = world
            .get::<TickStart>(target)
            .map_or(DVec3::ZERO, |d| d.moved);
        let relative = speed - (look.dot(target_delta) * f64::from(TICKS_PER_SECOND)) as f32;
        let damages = settings
            .damage
            .passes(speed, relative, elapsed, settings.delay);
        let knocks = settings
            .knockback
            .passes(speed, relative, elapsed, settings.delay);
        if !damages && !knocks {
            continue;
        }
        let damage = if damages {
            let bonus = (relative * settings.damage_multiplier).floor().max(0.0);
            attack_damage_with(world, holder, &attack, bonus)
        } else {
            0.0
        };
        let hit = Hit {
            damage,
            cause: ActorDamageCause::EntityAttack,
            attacker: Some(holder),
            by_player: false,
            knockback_from: knocks.then_some(feet),
            knockback_power: knockback_power_of(world, holder),
        };
        let dealt = if world.get::<Player>(target).is_some() {
            hurt_player(world, target, &hit)
        } else {
            hurt_mob(world, target, hit)
        };
        if dealt <= 0.0 {
            continue;
        }
        if let Some(mut raised) = world.get_mut::<DealKineticDamage>(holder) {
            match raised.hits.iter_mut().find(|(hit, _)| *hit == target) {
                Some(entry) => entry.1 = now,
                None => raised.hits.push((target, now)),
            }
        }
        send_actor_event(
            world,
            holder,
            EnumsActorEvent::KineticDamageDealt,
            actor_unique_id(world, target) as i32,
        );
    }
}

/// The unit vector a body looks along at `yaw` and `pitch` degrees.
fn look_direction(yaw: f32, pitch: f32) -> DVec3 {
    let (yaw, pitch) = (f64::from(yaw).to_radians(), f64::from(pitch).to_radians());
    DVec3::new(
        -yaw.sin() * pitch.cos(),
        -pitch.sin(),
        yaw.cos() * pitch.cos(),
    )
}

#[cfg(test)]
mod tests {
    use unastar_data::item_components::FloatRange;

    use super::*;

    fn iron_spear() -> KineticWeapon {
        let conditions = |max_duration, min_speed, min_relative_speed| KineticConditions {
            max_duration,
            min_speed,
            min_relative_speed,
        };
        KineticWeapon {
            delay: 12,
            reach: FloatRange { min: 2.0, max: 4.5 },
            creative_reach: FloatRange { min: 2.0, max: 7.5 },
            hitbox_margin: 0.25,
            damage_multiplier: 0.95,
            damage_conditions: conditions(225, None, Some(4.6)),
            knockback_conditions: conditions(135, Some(5.1), None),
            dismount_conditions: conditions(50, Some(11.0), None),
        }
    }

    /// The zombie's goal halves the spear's reach and takes a fifth of its
    /// minimum speeds; the weapon stays raised 225 + 12 ticks.
    #[test]
    fn a_zombies_iron_spear_reaches_one_to_two_and_a_quarter_blocks() {
        let settings = KineticSettings::new(&iron_spear(), 0.5, 0.2);
        assert_eq!((settings.reach_min, settings.reach_max), (1.0, 2.25));
        assert_eq!(settings.mid_reach(), 1.625);
        assert_eq!(settings.done_after(), Some(237));
        // Walking at 2.28 blocks per second deals damage and knocks back;
        // standing still does neither.
        assert!(settings.damage.passes(2.28, 2.28, 100, 12));
        assert!(settings.knockback.passes(2.28, 2.28, 100, 12));
        assert!(!settings.damage.passes(0.0, 0.0, 100, 12));
        // The knockback condition runs out before the damage condition.
        assert!(!settings.knockback.passes(2.28, 2.28, 147, 12));
        assert!(settings.damage.passes(2.28, 2.28, 236, 12));
        assert!(!settings.damage.passes(2.28, 2.28, 237, 12));
    }
}
