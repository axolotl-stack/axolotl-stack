//! `minecraft:behavior.ranged_attack`: hold position in range of the target
//! and shoot it on a reload timer.
//!
//! The goal runs while the mob has a target and a `minecraft:shooter`. It
//! sets `FACING_TARGET_TO_RANGE_ATTACK` while it runs (BDS 1.26.51 sets the
//! flag in the batch of the `TARGET` actor data and clears it with the
//! target). Each tick it looks at the target and counts how long it has
//! seen it; once the target is in range and seen for `target_in_sight_time`
//! it stops walking, else it walks toward it. The reload is the authored
//! interval (scaled between its bounds by the target's distance over the
//! range) plus [`RANGED_RELOAD_EXTRA_TICKS`]; the first shot waits one tick
//! more. When the timer runs out the mob shoots if it sees the target in
//! range, else it restarts the reload. The shot itself is made after the
//! mob's tick (`mob::combat::ranged`).

use super::goals::GoalContext;
use super::targeting::Candidate;
use crate::entity::mob::components::{RangedAttackGoal, ShooterConfig};
use crate::entity::mob::constants::RANGED_RELOAD_EXTRA_TICKS;

/// Runtime state of the ranged goal.
#[derive(Debug, Default)]
pub struct RangedState {
    /// Ticks until the next shot; `None` until the first tick of a run.
    cooldown: Option<i32>,
    /// Consecutive ticks the target has been in sight.
    seen_ticks: i32,
}

impl GoalContext<'_> {
    fn ranged_target(&self) -> Option<Candidate> {
        Candidate::of(self.world, self.states.targets.target?)
    }

    pub(super) fn ranged_can_start(&mut self) -> bool {
        self.config::<RangedAttackGoal>().is_some()
            && self.config::<ShooterConfig>().is_some()
            && self.ranged_target().is_some()
    }

    pub(super) fn ranged_start(&mut self) {
        self.states.ranged = RangedState::default();
        self.effects.ranged_facing = Some(true);
    }

    pub(super) fn ranged_stop(&mut self) {
        self.states.ranged = RangedState::default();
        self.effects.ranged_facing = Some(false);
        self.nav.stop(self.movement);
    }

    /// Reload ticks at `distance` from the target.
    fn ranged_reload(goal: &RangedAttackGoal, distance: f64) -> i32 {
        let range = f64::from(goal.range_max);
        let t = if range > 0.0 { distance / range } else { 0.0 };
        let min = f64::from(goal.interval_min_ticks);
        let max = f64::from(goal.interval_max_ticks);
        (min + t * (max - min)).floor() as i32 + RANGED_RELOAD_EXTRA_TICKS
    }

    pub(super) fn ranged_tick(&mut self) {
        let Some(goal) = self.config::<RangedAttackGoal>() else {
            return;
        };
        let Some(target) = self.ranged_target() else {
            return;
        };
        let distance = self.body.feet.distance(target_feet(&target));
        let seen = self.sees(&target);
        let ranged = &mut self.states.ranged;
        ranged.seen_ticks = if seen { ranged.seen_ticks + 1 } else { 0 };
        let in_range =
            distance <= f64::from(goal.range_max) && distance >= f64::from(goal.range_min);
        if in_range && self.states.ranged.seen_ticks >= goal.sight_ticks {
            self.nav.stop(self.movement);
        } else if self.nav.is_done() && self.path_toward(target_feet(&target)) {
            self.follow_staged(goal.speed_multiplier);
        }
        self.look.look_at(target.eye, goal.y_max_head_rotation);
        let reload = Self::ranged_reload(goal, distance);
        let ranged = &mut self.states.ranged;
        let Some(cooldown) = ranged.cooldown else {
            ranged.cooldown = Some(reload + 1);
            return;
        };
        let cooldown = cooldown - 1;
        if cooldown > 0 {
            ranged.cooldown = Some(cooldown);
            return;
        }
        if seen && in_range {
            ranged.cooldown = Some(reload);
            self.effects.shoot = Some(target.entity);
        } else {
            ranged.cooldown = Some(reload);
        }
    }
}

/// The bottom centre of the target's box (a player's position is its eye).
pub(crate) fn target_feet(target: &Candidate) -> glam::DVec3 {
    glam::DVec3::new(
        (target.aabb.min.x + target.aabb.max.x) / 2.0,
        target.aabb.min.y,
        (target.aabb.min.z + target.aabb.max.z) / 2.0,
    )
}
