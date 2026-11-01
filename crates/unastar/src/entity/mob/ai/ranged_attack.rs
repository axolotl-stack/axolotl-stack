//! `minecraft:behavior.ranged_attack`: keep the target in range and shoot
//! it with the mob's `minecraft:shooter` (the shot itself is
//! `mob::combat::shoot`).
//!
//! The goal runs while the mob has a living target and is not holding an
//! uncharged crossbow (it keeps running while a path is left); it sets
//! `FACING_TARGET_TO_RANGE_ATTACK` while it runs, the client's cue to draw
//! the bow. Each tick it measures the squared distance from its feet to the
//! target's position at the bottom of the target's box and counts the ticks
//! it has seen the target (reset when unseen). Out of `attack_radius`, or
//! seen for less than `target_in_sight_time`, it walks toward the target;
//! in range and seen long enough it stops and turns its body to face it
//! (`follow_target` keeps walking until within `attack_radius_min` +
//! [`RANGED_FOLLOW_STOP_MARGIN`]); closer than `attack_radius_min` it backs
//! off. It always looks at the target.
//!
//! The reload timer counts down whether or not the target is seen, and
//! while the goal runs on with its target lost (only the counting is done
//! then; matches BDS 1.26.51 `skeleton-bow-field-hard`). When it
//! reaches 0 a burst of `burst_shots` is armed; each shot of the burst
//! waits until the target is seen, within `[attack_radius_min,
//! attack_radius]` and inside `ranged_fov`, `burst_interval` apart. A shot
//! sets the reload to `attack_interval_min + (max − min) × distance /
//! attack_radius` ticks, so a reload of `n` ticks shoots every `n + 1`. The
//! first tick of the goal with no reload left starts one. The goal's first
//! tick comes the tick after it starts, so its first shot is one tick later
//! than its cadence (matches BDS 1.26.51). The counters are not reset when
//! the goal stops (matches BDS 1.26.51), only when the goal composition is
//! rebuilt.

use glam::DVec3;

use super::control::yaw_toward;
use super::goals::GoalContext;
use super::targeting::Candidate;
use crate::entity::mob::components::WalkNavigationConfig;
use crate::entity::mob::components::ranged_attack::RangedAttackGoal;
use crate::entity::mob::constants::{RANGED_BACK_OFF_MARGIN, RANGED_FOLLOW_STOP_MARGIN};
use crate::server::game::item_use::ranged::{Drawn, charged, drawn};

/// Runtime state of the ranged attack goal.
#[derive(Debug, Default)]
pub struct RangedState {
    /// Ticks until the next burst is armed.
    attack_time: i32,
    /// Consecutive ticks the target was seen.
    see_time: i32,
    /// Shots left in the armed burst, and ticks until the next one.
    burst_remaining: i32,
    burst_timer: i32,
    /// The goal started this tick: its first tick is the next one.
    started: bool,
}

impl RangedState {
    /// The goal was (re)built: every counter starts at 0.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

impl<'a> GoalContext<'a> {
    fn ranged_config(&self) -> Option<&'a RangedAttackGoal> {
        self.config::<RangedAttackGoal>()
    }

    fn ranged_target(&self) -> Option<Candidate> {
        Candidate::of(self.world, self.states.targets.target?)
    }

    /// An uncharged crossbow in the main hand keeps the goal from running.
    fn holds_uncharged_crossbow(&self) -> bool {
        crate::entity::mob::gear::mainhand(self.world, self.entity)
            .is_some_and(|stack| drawn(stack.item_id()) == Some(Drawn::Crossbow) && !charged(stack))
    }

    pub(super) fn ranged_can_start(&mut self) -> bool {
        let Some(goal) = self.ranged_config() else {
            return false;
        };
        if self.config::<WalkNavigationConfig>().is_none() || self.holds_uncharged_crossbow() {
            return false;
        }
        let Some(target) = self.ranged_target() else {
            return false;
        };
        if goal.set_persistent && target.player {
            self.effects.persistent = true;
        }
        true
    }

    pub(super) fn ranged_can_continue(&mut self) -> bool {
        self.ranged_can_start() || !self.nav.is_done()
    }

    pub(super) fn ranged_start(&mut self) {
        self.states.ranged.started = true;
        self.effects.facing_target = Some(true);
    }

    pub(super) fn ranged_stop(&mut self) {
        self.states.ranged.see_time = 0;
        self.states.ranged.started = false;
        self.effects.facing_target = Some(false);
    }

    pub(super) fn ranged_tick(&mut self) {
        if std::mem::take(&mut self.states.ranged.started) {
            return;
        }
        let Some(goal) = self.ranged_config() else {
            return;
        };
        let Some(target) = self.ranged_target() else {
            // A target lost while the goal still runs (its path is not
            // done): the reload and the burst keep counting down.
            let state = &mut self.states.ranged;
            if state.burst_remaining > 0 {
                state.burst_timer -= 1;
            } else if state.attack_time > 0 {
                state.attack_time -= 1;
                if state.attack_time == 0 {
                    state.burst_remaining = goal.burst_shots;
                    state.burst_timer = 0;
                }
            }
            return;
        };
        let feet = self.body.feet;
        let aim = DVec3::new(target.pos.x, target.aabb.min.y, target.pos.z);
        let distance_sq = feet.distance_squared(aim);
        let seen = self.sees(&target);
        let state = &mut self.states.ranged;
        state.see_time = if seen { state.see_time + 1 } else { 0 };
        let see_time = state.see_time;

        let radius_sq = goal.radius * goal.radius;
        let min_sq = goal.radius_min * goal.radius_min;
        if distance_sq >= min_sq {
            let follow_stop = if goal.radius_min > 0.0 {
                goal.radius_min + RANGED_FOLLOW_STOP_MARGIN
            } else {
                0.0
            };
            if distance_sq > radius_sq
                || see_time < goal.see_ticks
                || (goal.follow_target && distance_sq > follow_stop * follow_stop)
            {
                self.approach(&target, goal.speed_multiplier);
            } else {
                self.nav.stop(self.movement);
                self.effects.face = Some(yaw_toward(target.pos - feet));
            }
        } else {
            // Back off to (sin a, cos a) × the shortfall, a the angle away
            // from the target (matches BDS 1.26.51).
            let back = goal.radius_min - distance_sq.sqrt() + RANGED_BACK_OFF_MARGIN;
            let angle = (feet.z - target.pos.z).atan2(feet.x - target.pos.x);
            let point = DVec3::new(
                feet.x + angle.sin() * back,
                feet.y,
                feet.z + angle.cos() * back,
            );
            if self.path_toward(point) {
                self.follow_staged(goal.speed_multiplier);
            }
        }
        self.look.look_at(target.eye, goal.y_max_head_rotation);

        let fraction = if goal.radius > 0.0 {
            distance_sq.sqrt() / goal.radius
        } else {
            0.0
        };
        let reload = (f64::from(goal.interval_min_ticks)
            + f64::from(goal.interval_max_ticks - goal.interval_min_ticks) * fraction)
            .floor() as i32;
        let state = &mut self.states.ranged;
        if state.burst_remaining > 0 {
            state.burst_timer -= 1;
            if !seen || distance_sq > radius_sq || distance_sq < min_sq || state.burst_timer > 0 {
                return;
            }
            if !self.within_fov(target.pos, goal.ranged_fov) {
                return;
            }
            let state = &mut self.states.ranged;
            state.attack_time = reload;
            state.burst_remaining -= 1;
            if state.burst_remaining > 0 {
                state.burst_timer = goal.burst_interval_ticks;
            }
            self.effects.shot = Some(target.entity);
            self.effects.swing |= goal.swing;
            return;
        }
        let old = state.attack_time;
        state.attack_time -= 1;
        if state.attack_time == 0 {
            state.burst_remaining = goal.burst_shots;
            state.burst_timer = 0;
        } else if old <= 0 {
            state.attack_time = reload;
        }
    }

    /// Walk toward the target: a new path when none is followed or the
    /// target left the followed path's end block.
    fn approach(&mut self, target: &Candidate, speed_multiplier: f32) {
        let wanted = DVec3::new(target.pos.x, target.aabb.min.y, target.pos.z).floor();
        let on_path = self
            .nav
            .path()
            .last()
            .is_some_and(|waypoint| waypoint.feet.floor() == wanted);
        if !self.nav.is_done() && on_path {
            self.nav.set_speed_multiplier(speed_multiplier);
            return;
        }
        if self.path_to_target(target) {
            self.follow_staged(speed_multiplier);
        }
    }
}
