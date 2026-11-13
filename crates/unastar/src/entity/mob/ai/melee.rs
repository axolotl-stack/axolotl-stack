//! `minecraft:behavior.melee_box_attack` and `minecraft:behavior.stomp_attack`:
//! chase the target and hit it when it is within the goal's [`MeleeReach`].
//!
//! The goal runs while the mob has a target (set by the target goals) and
//! a path to it. Each tick it looks at the target, counts down the attack
//! cooldown and re-paths when its path timer runs out: a random
//! `[min_path_time, max_path_time)` plus the boundary increases for far
//! targets, `path_fail_time_increase` more when no path is found. Without
//! `track_target` it only re-paths while it sees the target. A hit needs
//! the target in reach, the cooldown run out and the target inside the
//! `melee_fov` cone; the hit itself is dealt after the mob's tick
//! (`mob::combat::melee`). The cooldown starts full when the goal is
//! built, as the native goal is constructed with it.
//!
//! The stomp reach decides after the move instead (`tick_stomp_attacks`,
//! after the physics step): BDS 1.26.51's polar bear hit, reared up and
//! restarted its cooldown by its own position after that tick's move and
//! its target's position before it ([`StompClock`]).

use bevy_ecs::prelude::*;
use glam::DVec3;
use rand::Rng;

use super::goals::GoalContext;
use super::targeting::Candidate;
use crate::entity::mob::components::{AttackConfig, MeleeAttackGoal, MeleeReach};
use crate::entity::mob::constants::{MELEE_REPATH_CHANCE, STOMP_WARNING_TICKS};
use crate::entity::mob::state::{Dead, MeleeAttackedOnce};
use crate::world::rules::Difficulty;

/// Runtime state of the melee goal.
#[derive(Debug, Default)]
pub struct MeleeState {
    /// Ticks until the next hit may land; `None` until the goal is built.
    attack_ticks: Option<i32>,
    /// Ticks until the path to the target is recomputed.
    path_timer: i32,
    /// The target position the current path was computed for (x, box
    /// bottom, z); zero before the first path.
    last_target: DVec3,
    /// The stomp reach's clock while a stomp goal runs.
    pub(super) stomp: Option<StompClock>,
}

/// The cooldown of a stomp goal ([`MeleeReach::Stomp`]), stepped once a
/// tick after the mob moved. The first step after the goal starts leaves
/// the full cooldown; every later one counts it down to 0 first. Then, by
/// the squared distance from the mob's feet to the target's:
///
/// - within `reach_sq` with the cooldown run out: hit, restart the cooldown
///   and stand down;
/// - else within `no_damage_sq`: a cooldown that ran out restarts (the mob
///   stands down), and with [`STOMP_WARNING_TICKS`] or fewer left the mob
///   rears up;
/// - else: the mob stands down.
///
/// Every rear-up, stand-down and hit of five of BDS 1.26.51's six measured
/// hostile polar bears follows this (`bds-1.26.51-polar_bear.json`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StompClock {
    ticks: i32,
    standing: bool,
    /// The full cooldown the clock started with.
    full: i32,
}

/// What one [`StompClock`] step decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StompStep {
    pub hit: bool,
    /// The `STANDING` flag changed to this value.
    pub standing: Option<bool>,
}

impl StompClock {
    /// The clock of a goal that just started, with a full `cooldown`.
    pub fn new(cooldown: i32) -> Self {
        Self {
            ticks: cooldown + 1,
            standing: false,
            full: cooldown,
        }
    }

    pub fn standing(&self) -> bool {
        self.standing
    }

    pub fn step(&mut self, distance_sq: f64, reach_sq: f64, no_damage_sq: f64) -> StompStep {
        let cooldown = self.full;
        self.ticks = (self.ticks - 1).max(0);
        let was = self.standing;
        let mut hit = false;
        if distance_sq <= reach_sq && self.ticks == 0 {
            hit = true;
            self.ticks = cooldown;
            self.standing = false;
        } else if distance_sq <= no_damage_sq {
            if self.ticks == 0 {
                self.ticks = cooldown;
                self.standing = false;
            }
            if self.ticks <= STOMP_WARNING_TICKS {
                self.standing = true;
            }
        } else {
            self.standing = false;
        }
        StompStep {
            hit,
            standing: (self.standing != was).then_some(self.standing),
        }
    }
}

impl MeleeReach {
    /// The stomp reach's squared hit and no-damage distances for a mob
    /// `width` wide and a target `target_width` wide: `(width ×
    /// reach_multiplier)² + target_width`, and that times
    /// `no_damage_range_multiplier`. `None` for the box reach.
    pub fn stomp_ranges(&self, width: f64, target_width: f64) -> Option<(f64, f64)> {
        match *self {
            MeleeReach::Box { .. } => None,
            MeleeReach::Stomp {
                reach_multiplier,
                no_damage_range_multiplier,
            } => {
                let reach = width * reach_multiplier;
                let reach_sq = reach * reach + target_width;
                Some((reach_sq, reach_sq * no_damage_range_multiplier))
            }
        }
    }
}

impl MeleeState {
    /// The goal was (re)built: the cooldown starts full again, and a
    /// running stomp clock starts over. Returns whether the mob was reared
    /// up, so the caller lowers `STANDING`.
    pub fn reset(&mut self) -> bool {
        let stomp = self.stomp.take();
        *self = Self::default();
        self.stomp = stomp.map(|clock| StompClock::new(clock.full));
        stomp.is_some_and(|clock| clock.standing)
    }
}

impl GoalContext<'_> {
    fn melee_target(&self) -> Option<Candidate> {
        Candidate::of(self.world, self.states.targets.target?)
    }

    /// The ground under the target's box centre.
    fn target_ground(target: &Candidate) -> DVec3 {
        let aabb = target.aabb;
        DVec3::new(
            (aabb.min.x + aabb.max.x) / 2.0,
            aabb.min.y,
            (aabb.min.z + aabb.max.z) / 2.0,
        )
    }

    pub(super) fn melee_can_start(&mut self) -> bool {
        if self.config::<MeleeAttackGoal>().is_none()
            || self.config::<AttackConfig>().is_none()
            || self.world.get::<MeleeAttackedOnce>(self.entity).is_some()
        {
            return false;
        }
        let Some(target) = self.melee_target() else {
            return false;
        };
        self.path_toward(Self::target_ground(&target))
    }

    pub(super) fn melee_start(&mut self) {
        let Some(goal) = self.config::<MeleeAttackGoal>() else {
            return;
        };
        let (speed, cooldown, fail) = (
            goal.speed_multiplier,
            goal.cooldown_ticks,
            goal.path_fail_ticks,
        );
        let stomp = matches!(goal.reach, MeleeReach::Stomp { .. });
        self.follow_staged(speed);
        let melee = &mut self.states.melee;
        melee.attack_ticks.get_or_insert(cooldown);
        melee.path_timer = fail;
        melee.stomp = stomp.then(|| StompClock::new(cooldown));
    }

    pub(super) fn melee_can_continue(&mut self) -> bool {
        let Some(goal) = self.config::<MeleeAttackGoal>() else {
            return false;
        };
        if self.config::<AttackConfig>().is_none()
            || self.world.get::<MeleeAttackedOnce>(self.entity).is_some()
        {
            return false;
        }
        if goal.random_stop_interval > 0 && self.rng.gen_range(0..goal.random_stop_interval) == 0 {
            self.set_target(None);
            return false;
        }
        let Some(target) = self.melee_target() else {
            return false;
        };
        if target.player
            && self
                .world
                .get_resource::<Difficulty>()
                .copied()
                .unwrap_or_default()
                == Difficulty::Peaceful
        {
            self.set_target(None);
            return false;
        }
        goal.track_target || !self.nav.is_done()
    }

    pub(super) fn melee_stop(&mut self) {
        self.nav.stop(self.movement);
        self.states.melee.last_target = DVec3::ZERO;
        if self
            .states
            .melee
            .stomp
            .take()
            .is_some_and(|clock| clock.standing())
        {
            self.effects.standing = Some(false);
        }
    }

    pub(super) fn melee_tick(&mut self) {
        let Some(goal) = self.config::<MeleeAttackGoal>() else {
            return;
        };
        let Some(target) = self.melee_target() else {
            return;
        };
        let dx = target.pos.x - self.body.feet.x;
        let dz = target.pos.z - self.body.feet.z;
        let MeleeReach::Box { horizontal: reach } = goal.reach else {
            // The stomp reach decides after the move (`tick_stomp_attacks`).
            self.move_toward_target(goal, &target, false, dx * dx + dz * dz);
            return;
        };
        let melee = &mut self.states.melee;
        let previous = *melee.attack_ticks.get_or_insert(goal.cooldown_ticks);
        if previous > 0 {
            melee.attack_ticks = Some(previous - 1);
        }
        let own = self.aabb();
        let attack_box = crate::registry::collision::Aabb::new(
            own.min - DVec3::new(reach, 0.0, reach),
            own.max + DVec3::new(reach, 0.0, reach),
        );
        let in_reach = previous < 2 && attack_box.intersects(&target.aabb);
        self.move_toward_target(goal, &target, in_reach, dx * dx + dz * dz);
        if in_reach && self.within_fov(goal, &target) {
            self.states.melee.attack_ticks = Some(goal.cooldown_ticks);
            self.effects.attack = Some(target.entity);
        }
    }

    /// Look at the target and count down the path timer. Unless the target
    /// is unseen and the goal does not track it, re-path to the target's
    /// ground position when [`Self::should_repath`] says so, and reset the
    /// timer to a random value between the goal's bounds plus the boundary
    /// penalty for distant targets and the failure penalty when no path is
    /// found.
    fn move_toward_target(
        &mut self,
        goal: &MeleeAttackGoal,
        target: &Candidate,
        in_reach: bool,
        distance_sq: f64,
    ) {
        self.look.look_at(target.eye, goal.y_max_head_rotation);
        self.states.melee.path_timer -= 1;
        if !goal.track_target && !self.sees(target) {
            return;
        }
        let previous = self.states.melee.last_target;
        let wanted = DVec3::new(target.pos.x, target.aabb.min.y, target.pos.z);
        self.states.melee.last_target = wanted;
        if !self.should_repath(in_reach, previous, wanted) {
            return;
        }
        let spread = goal.max_path_ticks - goal.min_path_ticks;
        let mut timer = goal.min_path_ticks;
        if spread > 0 {
            timer += self.rng.gen_range(0..spread);
        }
        if distance_sq > goal.outer_boundary_sq {
            timer += goal.outer_boundary_ticks;
        } else if distance_sq > goal.inner_boundary_sq {
            timer += goal.inner_boundary_ticks;
        }
        if self.path_toward(Self::target_ground(target)) {
            self.follow_staged(goal.speed_multiplier);
        } else {
            timer += goal.path_fail_ticks;
        }
        self.states.melee.path_timer = timer;
    }

    /// Whether to re-path now: when the timer ran out, re-path
    /// if there is no path, the first time, when the target left the path's
    /// end block (unless already in reach), or with a small chance anyway.
    fn should_repath(&mut self, in_reach: bool, previous: DVec3, wanted: DVec3) -> bool {
        if self.states.melee.path_timer > 0 {
            return false;
        }
        if previous == DVec3::ZERO {
            return true;
        }
        let Some(end) = self
            .nav
            .current()
            .and_then(|(_, waypoints)| waypoints.last())
            .map(|waypoint| waypoint.feet)
        else {
            return true;
        };
        let end = end.floor();
        let wanted = wanted.floor();
        if end.x != wanted.x || end.z != wanted.z || end.y != wanted.y {
            return !in_reach;
        }
        self.rng.r#gen::<f32>() < MELEE_REPATH_CHANCE
    }

    /// The target's box centre lies within `melee_fov` degrees of the
    /// mob's look direction.
    fn within_fov(&self, goal: &MeleeAttackGoal, target: &Candidate) -> bool {
        let Some(fov) = goal.melee_fov else {
            return true;
        };
        let centre = (target.aabb.min + target.aabb.max) / 2.0;
        let to = centre - self.self_eye();
        let length = to.length();
        if length < 1.0e-4 {
            return true;
        }
        let yaw = f64::from(self.head_yaw).to_radians();
        let look = DVec3::new(-yaw.sin(), 0.0, yaw.cos());
        let cos = (look.dot(to) / length).clamp(-1.0, 1.0);
        cos.acos().to_degrees() < f64::from(fov)
    }
}

/// System, after the physics step: step the clock of every running stomp
/// goal by the mob's position after this tick's move and its target's
/// feet (a player's as its client last reported them; a mob target has
/// moved this tick too, which no capture measured); a hit is dealt now (`mob::combat::melee`) and the `STANDING` flag
/// follows the clock.
pub fn tick_stomp_attacks(world: &mut World, mut mobs: Local<Vec<(Entity, Entity, StompStep)>>) {
    use crate::entity::components::{PhysicsBody, Position};
    mobs.clear();
    let mut query = world
        .query_filtered::<(Entity, &super::MobBrain, &Position, &PhysicsBody), Without<Dead>>();
    let mut steps = Vec::new();
    for (entity, brain, position, body) in query.iter(world) {
        if brain.states.melee.stomp.is_some() {
            steps.push((entity, brain.states.targets.target, position.0, body.width));
        }
    }
    for (entity, target, feet, width) in steps {
        let Some(target) = target.and_then(|target| Candidate::of(world, target)) else {
            continue;
        };
        let Some(goal) = world.get::<MeleeAttackGoal>(entity) else {
            continue;
        };
        let target_width = target.aabb.max.x - target.aabb.min.x;
        let Some((reach_sq, no_damage_sq)) = goal.reach.stomp_ranges(width, target_width) else {
            continue;
        };
        let target_feet = DVec3::new(target.pos.x, target.aabb.min.y, target.pos.z);
        let distance_sq = feet.distance_squared(target_feet);
        let Some(mut brain) = world.get_mut::<super::MobBrain>(entity) else {
            continue;
        };
        let Some(clock) = brain.states.melee.stomp.as_mut() else {
            continue;
        };
        let step = clock.step(distance_sq, reach_sq, no_damage_sq);
        mobs.push((entity, target.entity, step));
    }
    for &(entity, target, step) in mobs.iter() {
        if let Some(standing) = step.standing {
            super::set_flag(
                &mut world.entity_mut(entity),
                jolyne::valentine::actor_flags::STANDING,
                standing,
            );
        }
        if step.hit {
            crate::entity::mob::combat::melee::melee_attack(world, entity, target);
        }
    }
}
