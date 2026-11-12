//! `minecraft:behavior.leap_at_target`: jump at the target.
//!
//! The goal may start while the mob has a target whose squared distance
//! from the mob's feet to the target's position (a player's eyes) lies in
//! [`LEAP_MIN_DISTANCE_SQ`]`..=`[`LEAP_MAX_DISTANCE_SQ`], it stands on the
//! ground when `must_be_on_ground`, and one roll in [`LEAP_CHANCE`] passes.
//! Starting sets the mob's velocity to [`LEAP_HORIZONTAL_SPEED`] along the
//! horizontal direction to the target and `yd` up, in place of the travel
//! step's; the goal then runs until the mob is on the ground again. It
//! needs the move control, which `behavior.melee_box_attack` (a higher
//! priority for the spider) holds while it chases, so here a chasing spider
//! never leaps. BDS 1.26.51 spiders leapt rarely: 4 times in 100 cycles of
//! the `leap` sweep (3 in the cycle the spider turned angry) and twice in
//! 10 chases, mid-air; when BDS lets a chasing spider leap is not modelled.

use glam::DVec3;
use rand::Rng;

use super::goals::GoalContext;
use super::targeting::Candidate;
use crate::entity::mob::components::LeapAtTargetGoal;
use crate::entity::mob::constants::{
    LEAP_CHANCE, LEAP_HORIZONTAL_SPEED, LEAP_MAX_DISTANCE_SQ, LEAP_MIN_DISTANCE_SQ,
};

/// The velocity a leap from `feet` at a target at `target` gives.
pub(super) fn leap_velocity(feet: DVec3, target: DVec3, yd: f32) -> DVec3 {
    let toward = DVec3::new(target.x - feet.x, 0.0, target.z - feet.z);
    let horizontal = toward.normalize_or_zero() * LEAP_HORIZONTAL_SPEED;
    DVec3::new(horizontal.x, f64::from(yd), horizontal.z)
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
        let (Some(goal), Some(target)) = (self.config::<LeapAtTargetGoal>(), self.leap_target())
        else {
            return;
        };
        self.effects.leap = Some(leap_velocity(self.body.feet, target.pos, goal.yd));
    }
}
