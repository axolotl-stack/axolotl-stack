//! The flying mobs' goal and move control (`minecraft:navigation.float`
//! with `minecraft:behavior.float_wander`, the bat), measured on BDS
//! 1.26.51 with `sweep.py bat` and replayed by `mob::tests::bat` against
//! `mob/ai/testdata/bds-1.26.51-bat.json`.
//!
//! A float-navigating mob never paths. `behavior.float_wander` wanders for
//! a duration drawn from `float_duration`, cut to whole ticks, plus one
//! (3 to 7 ticks for the bat). On each tick of a wander without a point it
//! draws one around the feet: `±xz_dist` on x and z, `y_offset ± y_dist`
//! on y, uniformly and not on the block grid. A point whose block is solid
//! is not kept, and that tick the mob does not push (BDS's bats never flew
//! at a point inside the floor, yet flew at points below the bottom of
//! the world). When the duration runs out the next wander starts on the
//! next tick.
//!
//! The control pushes the mob by its movement speed straight at the point,
//! from its feet before the move, on every tick it has one:
//! `v += speed × (point − feet) / |point − feet|`, walls and floors
//! included (the mob just bumps into them). The physics then moves it with
//! no gravity and drags it (`entity::mob::derive`). Its body and head face
//! the horizontal direction of the velocity it moves with.

use glam::{DVec3, IVec3};
use rand::Rng;

use super::control::{rotate_towards, wrap_degrees, yaw_toward};
use super::goals::GoalContext;
use crate::entity::mob::components::FloatWanderGoal;
use crate::entity::systems::physics::BlockSource;
use crate::registry::collision::block_collision_aabbs;

/// What the float control and `behavior.float_wander` keep between ticks.
#[derive(Debug, Default)]
pub struct FlyState {
    /// The point the current wander flies at, once a draw kept one.
    point: Option<DVec3>,
    /// Ticks left of the current wander; a new one starts at zero.
    ticks_left: u32,
    /// The point a goal flies at this tick.
    wanted: Option<DVec3>,
    /// The vex's course: held until it arrives (`super::vex`).
    course: Option<Course>,
}

/// A point the vex's move control flies to until it arrives, and the push
/// a tick toward it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Course {
    pub point: DVec3,
    pub push: f64,
}

/// The float control's result for one tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Flight {
    /// Body and head yaw.
    pub yaw: f32,
    pub velocity: DVec3,
}

impl FlyState {
    /// Run the control once the goals have ticked: push the mob at `speed`
    /// toward this tick's point, if a goal wants one, and face the motion.
    pub fn control(&mut self, speed: f64, feet: DVec3, yaw: f32, velocity: DVec3) -> Flight {
        let mut velocity = velocity;
        if let Some(point) = self.wanted.take() {
            let towards = point - feet;
            let distance = towards.length();
            if distance > 0.0 {
                velocity += towards * (speed / distance);
            }
        }
        let yaw = if velocity.x != 0.0 || velocity.z != 0.0 {
            wrap_degrees(yaw_toward(velocity))
        } else {
            yaw
        };
        Flight { yaw, velocity }
    }

    /// The vex's move control: on a tick its feet are closer than `arrive`
    /// to its course's point it keeps `slowdown` of its velocity and drops
    /// the course; on any other tick of a course it pushes `push` along the
    /// unit vector from its feet to the point. Its body turns at most
    /// `max_turn` a tick toward `facing` when it has one (its target), else
    /// toward the direction it moves this tick.
    #[allow(clippy::too_many_arguments)]
    pub fn steer(
        &mut self,
        feet: DVec3,
        arrive: f64,
        slowdown: f64,
        yaw: f32,
        max_turn: f32,
        facing: Option<DVec3>,
        velocity: DVec3,
    ) -> Flight {
        let mut velocity = velocity;
        if let Some(course) = self.course {
            let towards = course.point - feet;
            let distance = towards.length();
            if distance < arrive {
                velocity *= slowdown;
                self.course = None;
            } else {
                velocity += towards * (course.push / distance);
            }
        }
        let direction = facing.map_or(velocity, |point| point - feet);
        let yaw = if direction.x != 0.0 || direction.z != 0.0 {
            rotate_towards(yaw, yaw_toward(direction), max_turn)
        } else {
            yaw
        };
        Flight { yaw, velocity }
    }

    /// Whether the vex's control has no course.
    pub fn idle(&self) -> bool {
        self.course.is_none()
    }

    /// Set the vex's course.
    pub fn set_course(&mut self, course: Course) {
        self.course = Some(course);
    }

    /// The vex's course.
    pub fn course(&self) -> Option<Course> {
        self.course
    }

    /// The vex's course, to steer it again.
    pub(super) fn course_mut(&mut self) -> Option<&mut Course> {
        self.course.as_mut()
    }

    /// Forget the current wander: a bat that settles starts a new one when
    /// it wakes.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// The point the current wander flies at (tests).
    #[cfg(test)]
    pub fn point(&self) -> Option<DVec3> {
        self.point
    }

    /// Fly at `point` for `ticks` ticks (tests).
    #[cfg(test)]
    pub fn set_wander(&mut self, point: DVec3, ticks: u32) {
        self.point = Some(point);
        self.ticks_left = ticks;
    }
}

/// Whether `block` has a collision box; `None` when its chunk is not
/// loaded. Above and below the world the block is air.
pub(super) fn solid(blocks: &impl BlockSource, block: IVec3) -> Option<bool> {
    blocks
        .block_at(block)
        .map(|id| block_collision_aabbs(id).next().is_some())
}

/// Whether a wander point is not kept: its block is solid, or its chunk is
/// not loaded.
fn blocked(blocks: &impl BlockSource, point: DVec3) -> bool {
    solid(blocks, point.floor().as_ivec3()).unwrap_or(true)
}

impl GoalContext<'_> {
    pub(super) fn float_wander_can_start(&self) -> bool {
        self.config::<FloatWanderGoal>().is_some()
    }

    pub(super) fn float_wander_stop(&mut self) {
        let state = &mut self.states.fly;
        state.point = None;
        state.ticks_left = 0;
    }

    pub(super) fn float_wander_tick(&mut self) {
        let Some(goal) = self.config::<FloatWanderGoal>() else {
            return;
        };
        let feet = self.body.feet;
        let state = &mut self.states.fly;
        if state.ticks_left == 0 {
            state.ticks_left = goal.duration(self.rng.r#gen::<f32>());
            state.point = None;
        }
        if state.point.is_none() {
            let offset = DVec3::new(
                self.rng.gen_range(-goal.xz_dist..goal.xz_dist),
                goal.y_offset + self.rng.gen_range(-goal.y_dist..goal.y_dist),
                self.rng.gen_range(-goal.xz_dist..goal.xz_dist),
            );
            let point = feet + offset;
            if !blocked(self.world, point) {
                state.point = Some(point);
            }
        }
        state.wanted = state.point;
        state.ticks_left -= 1;
    }
}
