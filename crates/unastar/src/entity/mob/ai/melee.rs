//! `minecraft:behavior.melee_box_attack`: chase the target and hit it when
//! its box is within the mob's box grown by `horizontal_reach`.
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

use glam::DVec3;
use rand::Rng;

use super::goals::GoalContext;
use super::targeting::Candidate;
use crate::entity::mob::components::{AttackConfig, MeleeBoxAttackGoal};
use crate::entity::mob::constants::MELEE_REPATH_CHANCE;
use crate::entity::mob::state::MeleeAttackedOnce;
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
}

impl MeleeState {
    /// The goal was (re)built: the cooldown starts full again.
    pub fn reset(&mut self) {
        *self = Self::default();
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
        if self.config::<MeleeBoxAttackGoal>().is_none()
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
        let Some(goal) = self.config::<MeleeBoxAttackGoal>() else {
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
        let Some(goal) = self.config::<MeleeBoxAttackGoal>() else {
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
    }

    pub(super) fn melee_tick(&mut self) {
        let Some(goal) = self.config::<MeleeBoxAttackGoal>() else {
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
        let reach = goal.horizontal_reach;
        let own = self.aabb();
        let attack_box = crate::registry::collision::Aabb::new(
            own.min - DVec3::new(reach, 0.0, reach),
            own.max + DVec3::new(reach, 0.0, reach),
        );
        let in_reach = previous < 2 && attack_box.intersects(&target.aabb);
        let dx = target.pos.x - self.body.feet.x;
        let dz = target.pos.z - self.body.feet.z;
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
    fn within_fov(&self, goal: &MeleeBoxAttackGoal, target: &Candidate) -> bool {
        let centre = (target.aabb.min + target.aabb.max) / 2.0;
        let to = centre - self.self_eye();
        let length = to.length();
        if length < 1.0e-4 {
            return true;
        }
        let yaw = f64::from(self.head_yaw).to_radians();
        let look = DVec3::new(-yaw.sin(), 0.0, yaw.cos());
        let cos = (look.dot(to) / length).clamp(-1.0, 1.0);
        cos.acos().to_degrees() < f64::from(goal.melee_fov)
    }
}
