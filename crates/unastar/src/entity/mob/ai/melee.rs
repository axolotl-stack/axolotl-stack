//! `minecraft:behavior.melee_box_attack`: chase the target and hit it when
//! its box is within the mob's box grown by `horizontal_reach`.
//!
//! The goal runs while the mob has a target (set by the target goals) and
//! a path toward it; a route to the node closest to an unreachable target
//! is enough. Each tick it looks at the target, counts down the attack
//! cooldown and re-paths when its path timer runs out: a random
//! `[min_path_time, max_path_time)` plus the boundary increases for far
//! targets, `path_fail_time_increase` more when no path is found (a wall
//! climber instead heads straight for the target, `super::climb`).
//! Without `track_target` it only re-paths while it sees the target. A hit needs
//! the target in reach, the cooldown run out and the target inside the
//! `melee_fov` cone; the hit itself is dealt after the mob's tick
//! (`mob::combat::melee`). The cooldown starts full when the goal is
//! built, as the native goal is constructed with it.

use glam::DVec3;
use rand::Rng;

use super::goals::GoalContext;
use super::targeting::Candidate;
use crate::entity::mob::components::hostile::{MeleeReach, melee_goal};
use crate::entity::mob::components::{AttackConfig, MeleeBoxAttackGoal};
use crate::entity::mob::constants::{MELEE_ATTACK_MIN_WIDTH, MELEE_REPATH_CHANCE};
use crate::entity::mob::state::MeleeAttackedOnce;
use crate::world::rules::Difficulty;

/// Runtime state of the melee goal.
#[derive(Debug, Default)]
pub struct MeleeState {
    /// Ticks until the next hit may land; `None` until the goal is built.
    attack_ticks: Option<i32>,
    /// Ticks until the path to the target is recalculated.
    path_timer: i32,
}

impl MeleeState {
    /// The goal was (re)built: the cooldown starts full again.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

impl<'a> GoalContext<'a> {
    /// The melee goal the mob runs (`behavior.melee_box_attack`, else
    /// `behavior.melee_attack`).
    fn melee_config(&self) -> Option<&'a MeleeBoxAttackGoal> {
        melee_goal(self.world.entity(self.entity))
    }

    pub(super) fn melee_target(&self) -> Option<Candidate> {
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

    /// Search a path to the ground under `target` into the staging buffer.
    /// A route to the node closest to an unreachable target is enough; a
    /// wall climber records the target's block as its destination.
    pub(super) fn path_to_target(&mut self, target: &Candidate) -> bool {
        self.request_path(Self::target_ground(target), target.pos, true)
    }

    pub(super) fn melee_can_start(&mut self) -> bool {
        if self.melee_config().is_none()
            || self.config::<AttackConfig>().is_none()
            || self.world.get::<MeleeAttackedOnce>(self.entity).is_some()
        {
            return false;
        }
        let Some(target) = self.melee_target() else {
            return false;
        };
        self.path_to_target(&target)
    }

    pub(super) fn melee_start(&mut self) {
        let Some(goal) = self.melee_config() else {
            return;
        };
        let (speed, cooldown, fail) = (
            goal.speed_multiplier,
            goal.cooldown_ticks,
            goal.path_fail_ticks,
        );
        self.follow_staged(speed);
        let melee = &mut self.states.melee;
        melee.attack_ticks.get_or_insert(cooldown);
        melee.path_timer = fail;
    }

    pub(super) fn melee_can_continue(&mut self) -> bool {
        let Some(goal) = self.melee_config() else {
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
    }

    pub(super) fn melee_tick(&mut self) {
        let Some(goal) = self.melee_config() else {
            return;
        };
        let Some(target) = self.melee_target() else {
            return;
        };
        let melee = &mut self.states.melee;
        let previous = *melee.attack_ticks.get_or_insert(goal.cooldown_ticks);
        if previous > 0 {
            melee.attack_ticks = Some(previous - 1);
        }
        let own = self.aabb();
        let touches = match goal.reach {
            MeleeReach::Box { horizontal } => crate::registry::collision::Aabb::new(
                own.min - DVec3::new(horizontal, 0.0, horizontal),
                own.max + DVec3::new(horizontal, 0.0, horizontal),
            )
            .intersects(&target.aabb),
            MeleeReach::Multiplier { reach_multiplier } => {
                let width = self.body.width.max(MELEE_ATTACK_MIN_WIDTH);
                let target_width = target.aabb.max.x - target.aabb.min.x;
                let reach_sq = (reach_multiplier * width).powi(2) + target_width;
                let feet = DVec3::new(target.pos.x, target.aabb.min.y, target.pos.z);
                let delta = feet - self.body.feet;
                let overlap_y = own.min.y < target.aabb.max.y && target.aabb.min.y < own.max.y;
                delta.length_squared() <= reach_sq
                    || (overlap_y && delta.x * delta.x + delta.z * delta.z <= reach_sq)
            }
        };
        let in_reach = previous < 2 && touches;
        let dx = target.pos.x - self.body.feet.x;
        let dz = target.pos.z - self.body.feet.z;
        self.move_toward_target(goal, &target, in_reach, dx * dx + dz * dz);
        let centre = (target.aabb.min + target.aabb.max) / 2.0;
        if in_reach && self.within_fov(centre, goal.melee_fov) {
            self.states.melee.attack_ticks = Some(goal.cooldown_ticks);
            self.effects.attack = Some(target.entity);
        }
    }

    /// Look at the target and, when the path timer ran out, re-path toward it.
    fn move_toward_target(
        &mut self,
        goal: &MeleeBoxAttackGoal,
        target: &Candidate,
        in_reach: bool,
        distance_sq: f64,
    ) {
        self.look.look_at(target.eye, goal.y_max_head_rotation);
        self.states.melee.path_timer -= 1;
        if !goal.track_target && !self.sees(target) {
            return;
        }
        let wanted = DVec3::new(target.pos.x, target.aabb.min.y, target.pos.z);
        if !self.should_repath(in_reach, wanted) {
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
        if self.path_to_target(target) {
            self.follow_staged(goal.speed_multiplier);
        } else if self.policy.climbs_walls {
            // No path: a wall climber heads straight for the target
            // (`super::climb`) at the goal's speed, without the fail delay.
            self.nav.set_speed_multiplier(goal.speed_multiplier);
        } else {
            timer += goal.path_fail_ticks;
        }
        self.states.melee.path_timer = timer;
    }

    /// When the timer ran out, re-path
    /// when the target stands exactly at the world origin, without a path,
    /// when the target left the path's end block (unless already in reach),
    /// or with a small chance anyway. The target position is taken before
    /// the check (matches BDS 1.26.51), so a goal that just started keeps
    /// the path it started with.
    fn should_repath(&mut self, in_reach: bool, wanted: DVec3) -> bool {
        if self.states.melee.path_timer > 0 {
            return false;
        }
        if wanted == DVec3::ZERO {
            return true;
        }
        // The end of the last path, even once followed to its end.
        let Some(end) = self.nav.path().last().map(|waypoint| waypoint.feet) else {
            return true;
        };
        let end = end.floor();
        let wanted = wanted.floor();
        if end.x != wanted.x || end.z != wanted.z || end.y != wanted.y {
            return !in_reach;
        }
        self.rng.r#gen::<f32>() < MELEE_REPATH_CHANCE
    }

    /// `point` lies within `fov` degrees of the mob's look direction (its
    /// head yaw), seen from its eye.
    pub(super) fn within_fov(&self, point: DVec3, fov: f32) -> bool {
        let to = point - self.self_eye();
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
