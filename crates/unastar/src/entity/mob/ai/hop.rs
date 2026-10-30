//! Hopping mobs: the jumping move control of `minecraft:movement.jump` and
//! the goals that drive it, `minecraft:behavior.slime_float`,
//! `minecraft:behavior.slime_keep_on_jumping`,
//! `minecraft:behavior.slime_random_direction` and
//! `minecraft:behavior.slime_attack`.
//!
//! A mob with `movement.jump` does not follow paths: its goals only ask the
//! [`HopControl`] to move this tick (at a speed multiplier) and set the
//! direction it faces. Each tick the control turns the body toward that
//! direction by at most `max_turn`. Without a request the mob stops. With
//! one, in the air it keeps moving at `multiplier × movement`; on the ground
//! it stands still while its jump delay counts down, then jumps forward at
//! that speed and draws the next delay. The delay is drawn from
//! `jump_delay` (seconds, as whole ticks) on the mob's first tick and after
//! every jump, so a mob that becomes aggressive uses its new delay from the
//! next jump on. This matches BDS 1.26.51.
//!
//! The goals (matches BDS 1.26.51):
//! - float runs while the mob is in water or lava: each tick it jumps with
//!   `jump_chance_percentage` and moves at its speed;
//! - keep on jumping always runs and moves at its speed;
//! - random direction runs without a live target while the mob stands on
//!   the ground or is in a liquid, and faces a new direction
//!   `nextInt(turn_range)` every `min_change_direction_time` plus
//!   `nextInt(add_random_time_range)` seconds, starting at once;
//! - attack runs while the target lives, for at most
//!   `grow_tired_cooldown_time`: it turns toward the centre of the target's
//!   box by at most `y_max_rotation` per tick and moves at its speed.

use glam::DVec3;
use rand::Rng;
use rand::rngs::StdRng;

use super::control::{Body, MoveControl, rotate_towards, yaw_toward};
use super::goals::GoalContext;
use super::targeting::Candidate;
use crate::entity::mob::components::JumpMovementConfig;
use crate::entity::mob::components::slime_goals::{
    HopAttackGoal, HopFloatGoal, HopKeepJumpingGoal, HopRandomDirectionGoal,
};
use crate::entity::mob::constants::TICKS_PER_SECOND;

/// The jumping move control of a `minecraft:movement.jump` mob (module
/// docs).
#[derive(Debug, Default)]
pub struct HopControl {
    /// Ground ticks left before the next jump; drawn on the first tick. A
    /// counter, not a deadline: it only runs down on the ground.
    delay: Option<i32>,
    /// The speed multiplier a goal asked to move at this tick.
    wanted: Option<f32>,
    /// The body yaw the goals want to face.
    direction: Option<f32>,
}

impl HopControl {
    /// Move this tick at `speed_multiplier` times the movement speed.
    pub fn want(&mut self, speed_multiplier: f32) {
        self.wanted = Some(speed_multiplier);
    }

    /// Face `yaw` (degrees), turning by at most `max_turn` per tick.
    pub fn set_direction(&mut self, yaw: f32) {
        self.direction = Some(yaw);
    }

    /// Advance one tick: turn `yaw` (the body yaw), set the travel speed in
    /// `movement` and return whether the mob jumps now.
    pub fn tick(
        &mut self,
        config: &JumpMovementConfig,
        body: &Body,
        yaw: &mut f32,
        movement: &mut MoveControl,
        rng: &mut StdRng,
    ) -> bool {
        if let Some(direction) = self.direction {
            *yaw = rotate_towards(*yaw, direction, config.max_turn);
        }
        let delay = *self.delay.get_or_insert_with(|| jump_delay(config, rng));
        let Some(multiplier) = self.wanted.take() else {
            movement.speed = 0.0;
            return false;
        };
        let speed = multiplier * body.movement;
        if !body.on_ground {
            movement.speed = speed;
            return false;
        }
        if delay < 2 {
            self.delay = Some(jump_delay(config, rng));
            movement.speed = speed;
            true
        } else {
            self.delay = Some(delay - 1);
            movement.speed = 0.0;
            false
        }
    }
}

/// A jump delay drawn from `jump_delay`, in whole ticks.
fn jump_delay(config: &JumpMovementConfig, rng: &mut StdRng) -> i32 {
    let (min, max) = config.jump_delay;
    let seconds = if max > min {
        rng.gen_range(min..=max)
    } else {
        min
    };
    (seconds * TICKS_PER_SECOND) as i32
}

/// Runtime state of the hopping goals.
#[derive(Debug, Default)]
pub struct HopState {
    /// Server tick the random direction goal picks its next direction at.
    next_direction_tick: u64,
    /// The direction the random direction goal faces.
    direction: f32,
    /// Server tick the attack goal grows tired at.
    tired_tick: u64,
}

impl GoalContext<'_> {
    fn hop_target(&self) -> Option<Candidate> {
        Candidate::of(self.world, self.states.targets.target?)
    }

    pub(super) fn hop_float_can_continue(&mut self) -> bool {
        self.config::<HopFloatGoal>().is_some() && self.body.liquid.is_some()
    }

    pub(super) fn hop_float_tick(&mut self) {
        let Some(goal) = self.config::<HopFloatGoal>() else {
            return;
        };
        if self.rng.r#gen::<f32>() < goal.jump_chance {
            self.jump = true;
        }
        self.hop.want(goal.speed_multiplier);
    }

    pub(super) fn hop_keep_jumping_tick(&mut self) {
        if let Some(goal) = self.config::<HopKeepJumpingGoal>() {
            self.hop.want(goal.speed_multiplier);
        }
    }

    pub(super) fn hop_random_direction_can_continue(&mut self) -> bool {
        self.config::<HopRandomDirectionGoal>().is_some()
            && self.hop_target().is_none()
            && (self.body.on_ground || self.body.liquid.is_some())
    }

    pub(super) fn hop_random_direction_start(&mut self) {
        self.states.hop.next_direction_tick = self.tick;
    }

    pub(super) fn hop_random_direction_tick(&mut self) {
        let Some(goal) = self.config::<HopRandomDirectionGoal>() else {
            return;
        };
        let state = &mut self.states.hop;
        if self.tick >= state.next_direction_tick {
            let extra = if goal.add_random_ticks > 0 {
                self.rng.gen_range(0..goal.add_random_ticks)
            } else {
                0
            };
            state.next_direction_tick = self.tick + (goal.min_change_ticks + extra).max(0) as u64;
            state.direction = if goal.turn_range > 0 {
                self.rng.gen_range(0..goal.turn_range) as f32
            } else {
                0.0
            };
        }
        self.hop.set_direction(state.direction);
    }

    pub(super) fn hop_attack_can_start(&mut self) -> bool {
        self.config::<HopAttackGoal>().is_some() && self.hop_target().is_some()
    }

    pub(super) fn hop_attack_can_continue(&mut self) -> bool {
        self.hop_attack_can_start() && self.tick < self.states.hop.tired_tick
    }

    pub(super) fn hop_attack_start(&mut self) {
        let Some(goal) = self.config::<HopAttackGoal>() else {
            return;
        };
        self.states.hop.tired_tick = self.tick + goal.grow_tired_ticks.max(0) as u64;
        if goal.set_persistent && self.hop_target().is_some_and(|target| target.player) {
            self.effects.persistent = true;
        }
    }

    pub(super) fn hop_attack_tick(&mut self) {
        let (Some(goal), Some(target)) = (self.config::<HopAttackGoal>(), self.hop_target()) else {
            return;
        };
        let centre = (target.aabb.min + target.aabb.max) * 0.5;
        let delta: DVec3 = centre - self.body.feet;
        if delta.x * delta.x + delta.z * delta.z > 1.0e-7 {
            let yaw = rotate_towards(self.yaw, yaw_toward(delta), goal.y_max_rotation);
            self.hop.set_direction(yaw);
        }
        self.look.look_at(centre, goal.y_max_rotation);
        self.hop.want(goal.speed_multiplier);
    }
}

#[cfg(test)]
mod tests {
    use rand::SeedableRng;

    use super::*;

    fn body(on_ground: bool) -> Body {
        Body {
            feet: DVec3::ZERO,
            width: 0.52,
            height: 0.52,
            step_height: 0.5625,
            on_ground,
            liquid: None,
            movement: 0.3,
        }
    }

    const CONFIG: JumpMovementConfig = JumpMovementConfig {
        jump_delay: (0.5, 0.5),
        max_turn: 30.0,
    };

    #[test]
    fn grounded_hopper_waits_its_delay_then_jumps_forward() {
        let mut control = HopControl::default();
        let mut movement = MoveControl::default();
        let mut rng = StdRng::seed_from_u64(1);
        let mut yaw = 0.0;
        let mut jumps = Vec::new();
        for tick in 0..25 {
            control.want(1.0);
            if control.tick(&CONFIG, &body(true), &mut yaw, &mut movement, &mut rng) {
                jumps.push(tick);
                assert!((movement.speed - 0.3).abs() < 1e-6);
            } else {
                assert_eq!(movement.speed, 0.0, "stands still while waiting");
            }
        }
        // 0.5 s is 10 ticks: the jump comes on the 10th grounded tick.
        assert_eq!(jumps, [9, 19]);
    }

    #[test]
    fn airborne_hopper_keeps_moving_and_stops_without_a_request() {
        let mut control = HopControl::default();
        let mut movement = MoveControl::default();
        let mut rng = StdRng::seed_from_u64(1);
        let mut yaw = 0.0;
        control.want(1.2);
        assert!(!control.tick(&CONFIG, &body(false), &mut yaw, &mut movement, &mut rng));
        assert!((movement.speed - 0.36).abs() < 1e-6);
        assert!(!control.tick(&CONFIG, &body(false), &mut yaw, &mut movement, &mut rng));
        assert_eq!(movement.speed, 0.0);
    }

    #[test]
    fn hopper_turns_by_max_turn() {
        let mut control = HopControl::default();
        let mut movement = MoveControl::default();
        let mut rng = StdRng::seed_from_u64(1);
        let mut yaw = 0.0;
        control.set_direction(100.0);
        control.tick(&CONFIG, &body(true), &mut yaw, &mut movement, &mut rng);
        assert_eq!(yaw, 30.0);
        control.tick(&CONFIG, &body(true), &mut yaw, &mut movement, &mut rng);
        control.tick(&CONFIG, &body(true), &mut yaw, &mut movement, &mut rng);
        control.tick(&CONFIG, &body(true), &mut yaw, &mut movement, &mut rng);
        assert_eq!(yaw, 100.0);
    }
}
