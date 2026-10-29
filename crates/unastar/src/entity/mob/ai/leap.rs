//! `minecraft:behavior.leap_at_target`: jump at the target.
//!
//! The goal starts while the mob has a target whose squared distance (from
//! the mob's feet to the target's actor position, a player's eye) lies in
//! `[LEAP_MIN_DISTANCE_SQ, LEAP_MAX_DISTANCE_SQ]`, the mob stands on the
//! ground when `must_be_on_ground`, and then with a 1-in-[`LEAP_CHANCE`]
//! chance per tick; with `set_persistent` a player target makes the mob
//! persistent. Starting replaces the vertical velocity with `yd` and adds,
//! horizontally, [`LEAP_HORIZONTAL_SPEED`] along the direction to the target
//! plus [`LEAP_VELOCITY_RETAIN`] times the current horizontal velocity. The
//! goal runs until the mob is back on the ground. `target_dist` does not
//! change the leap. This matches BDS 1.26.51.

use glam::DVec3;
use rand::Rng;

use super::goals::GoalContext;
use super::targeting::Candidate;
use crate::entity::mob::components::leap::LeapAtTargetGoal;
use crate::entity::mob::constants::{
    LEAP_CHANCE, LEAP_HORIZONTAL_SPEED, LEAP_MAX_DISTANCE_SQ, LEAP_MIN_DISTANCE_SQ,
    LEAP_VELOCITY_RETAIN,
};

/// The leap a starting goal requests, applied to the mob's velocity before
/// its travel step.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Leap {
    /// Horizontal offset from the mob to its target (y is ignored).
    pub toward: DVec3,
    /// The new vertical velocity.
    pub yd: f64,
}

impl Leap {
    /// `velocity` after the leap.
    pub fn apply(self, velocity: DVec3) -> DVec3 {
        let mut out = velocity;
        let length = (self.toward.x * self.toward.x + self.toward.z * self.toward.z).sqrt();
        if length != 0.0 {
            out.x +=
                self.toward.x / length * LEAP_HORIZONTAL_SPEED + velocity.x * LEAP_VELOCITY_RETAIN;
            out.z +=
                self.toward.z / length * LEAP_HORIZONTAL_SPEED + velocity.z * LEAP_VELOCITY_RETAIN;
        }
        out.y = self.yd;
        out
    }
}

impl GoalContext<'_> {
    fn leap_target(&self) -> Option<Candidate> {
        Candidate::of(self.world, self.states.targets.target?)
    }

    pub(super) fn leap_can_start(&mut self) -> bool {
        let Some(goal) = self.config::<LeapAtTargetGoal>() else {
            return false;
        };
        let Some(target) = self.leap_target() else {
            return false;
        };
        let distance_sq = self.body.feet.distance_squared(target.pos);
        if !(LEAP_MIN_DISTANCE_SQ..=LEAP_MAX_DISTANCE_SQ).contains(&distance_sq) {
            return false;
        }
        if goal.must_be_on_ground && !self.body.on_ground {
            return false;
        }
        if self.rng.gen_range(0..LEAP_CHANCE) != 0 {
            return false;
        }
        if goal.set_persistent && target.player {
            self.effects.persistent = true;
        }
        true
    }

    pub(super) fn leap_start(&mut self) {
        let Some(goal) = self.config::<LeapAtTargetGoal>() else {
            return;
        };
        let Some(target) = self.leap_target() else {
            return;
        };
        self.effects.leap = Some(Leap {
            toward: target.pos - self.body.feet,
            yd: f64::from(goal.yd),
        });
    }

    pub(super) fn leap_can_continue(&self) -> bool {
        !self.body.on_ground
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leap_replaces_vertical_and_pushes_toward_the_target() {
        let leap = Leap {
            toward: DVec3::new(3.0, 5.0, 4.0),
            yd: 0.4,
        };
        let out = leap.apply(DVec3::new(0.1, -0.3, 0.0));
        assert!((out.x - (0.1 + 0.6 * 0.4 + 0.1 * 0.2)).abs() < 1e-12);
        assert!((out.z - 0.8 * 0.4).abs() < 1e-12);
        assert_eq!(out.y, 0.4);
        // Straight above: no horizontal push.
        let up = Leap {
            toward: DVec3::Y,
            yd: 0.4,
        }
        .apply(DVec3::ZERO);
        assert_eq!(up, DVec3::new(0.0, 0.4, 0.0));
    }
}
