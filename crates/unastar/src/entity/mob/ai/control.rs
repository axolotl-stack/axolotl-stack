//! Path following, the land move control, the look control and the jump
//! control of a mob.
//!
//! Goals hand paths to [`PathFollower`] and look targets to [`LookControl`].
//! Each tick the follower sets the move control's wanted position to the
//! next waypoint; the move control turns the body toward it, ramps the
//! speed and requests jumps; the look control turns the head; finally
//! [`travel`] converts speed and jump into velocity for the physics step.

use glam::DVec3;

use crate::entity::mob::constants::{
    JUMP_COOLDOWN_TICKS, LIQUID_JUMP_IMPULSE, LOOK_IDLE_HEAD_TURN, MOVE_ARRIVAL_FLOOR,
    MOVE_ARRIVE_DIST_SQ, MOVE_ARRIVE_HEIGHT, MOVE_DESCEND_CAP, MOVE_JUMP_HEIGHT_IN_WATER,
    MOVE_SPEED_RAMP, NORMAL_GROUND_FRICTION, PATH_STUCK_CHECK_TICKS, PATH_STUCK_DISTANCE_FACTOR,
    PATH_WAYPOINT_REACH, PATH_WAYPOINT_REACH_Y, WATER_MOVE_SPEED,
};
use crate::entity::navigation::{WalkNavigation, WalkWaypoint};
use crate::registry::block::LiquidKind;

/// What the controls know about the mob this tick.
#[derive(Debug, Clone, Copy)]
pub struct Body {
    pub feet: DVec3,
    pub width: f64,
    pub height: f64,
    pub step_height: f64,
    pub on_ground: bool,
    pub liquid: Option<LiquidKind>,
    /// Current value of the movement speed attribute.
    pub movement: f32,
}

impl Body {
    pub fn centre(&self) -> DVec3 {
        self.feet + DVec3::Y * (self.height * 0.5)
    }
}

/// Yaw (degrees) facing from the origin along `delta`: 0 faces +z, 90 faces −x.
pub fn yaw_toward(delta: DVec3) -> f32 {
    (delta.z.atan2(delta.x).to_degrees() - 90.0) as f32
}

/// Wrap an angle to `(-180, 180]` degrees.
pub fn wrap_degrees(angle: f32) -> f32 {
    let wrapped = angle.rem_euclid(360.0);
    if wrapped > 180.0 {
        wrapped - 360.0
    } else {
        wrapped
    }
}

/// Turn `from` toward `to` by at most `max` degrees.
pub fn rotate_towards(from: f32, to: f32, max: f32) -> f32 {
    let delta = wrap_degrees(to - from).clamp(-max, max);
    wrap_degrees(from + delta)
}

/// Unit forward vector of a body yaw.
fn forward(yaw: f32) -> DVec3 {
    let radians = f64::from(yaw).to_radians();
    DVec3::new(-radians.sin(), 0.0, radians.cos())
}

/// The land move control.
#[derive(Debug, Default)]
pub struct MoveControl {
    /// Position the mob walks toward.
    pub wanted: Option<DVec3>,
    pub speed_multiplier: f32,
    /// Whether `wanted` is the last node of the path (arrival slow-down).
    pub final_node: bool,
    /// Current speed; also the forward input of the travel step.
    pub speed: f32,
    pub jump_cooldown: u32,
}

impl MoveControl {
    pub fn set_wanted(&mut self, position: DVec3, speed_multiplier: f32, final_node: bool) {
        self.wanted = Some(position);
        self.speed_multiplier = speed_multiplier;
        self.final_node = final_node;
    }

    /// Advance one tick: update speed, turn `yaw` (the body yaw) and return
    /// whether a jump is requested.
    pub fn tick(&mut self, body: &Body, yaw: &mut f32, max_turn: f32) -> bool {
        let Some(wanted) = self.wanted else {
            self.speed = 0.0;
            return false;
        };
        let target = body.movement * self.speed_multiplier;
        // Distance is measured from the feet on the ground, from the box
        // centre in the air.
        let reference = if body.on_ground {
            body.feet
        } else {
            body.centre()
        };
        let distance_sq = (wanted - reference).length_squared();
        let rise = wanted.y - body.feet.y;
        if distance_sq < MOVE_ARRIVE_DIST_SQ && rise < MOVE_ARRIVE_HEIGHT {
            self.wanted = None;
            self.speed = 0.0;
            return false;
        }

        let previous = self.speed;
        let mut goal = target;
        if self.final_node && distance_sq < 1.0 {
            goal = (target * distance_sq as f32).max(MOVE_ARRIVAL_FLOOR * target);
        }
        let mut speed = if goal <= previous {
            goal
        } else {
            (previous + MOVE_SPEED_RAMP * target).min(goal)
        };
        if rise < 0.0 && speed > MOVE_DESCEND_CAP {
            speed = (speed - MOVE_SPEED_RAMP * target).max(MOVE_DESCEND_CAP);
        }
        self.speed = speed;

        let delta = wanted - body.feet;
        let horizontal_sq = delta.x * delta.x + delta.z * delta.z;
        let dead_zone = (body.width * 0.25).clamp(0.05, 0.25);
        if horizontal_sq > dead_zone * dead_zone {
            // A mob still accelerating may turn faster, up to twice max_turn.
            let limit = if previous < target {
                if previous > 0.0 {
                    (max_turn * target / previous).clamp(max_turn, 2.0 * max_turn)
                } else {
                    2.0 * max_turn
                }
            } else {
                max_turn
            };
            *yaw = rotate_towards(*yaw, yaw_toward(delta), limit);
        }

        let jump_height = if body.liquid.is_some() {
            MOVE_JUMP_HEIGHT_IN_WATER
        } else {
            body.step_height
        };
        rise > jump_height && horizontal_sq < body.width.max(1.0)
    }
}

/// Head look requests. A goal sets a target every tick it wants to look;
/// without one the head turns back toward the body.
#[derive(Debug, Default)]
pub struct LookControl {
    target: Option<(DVec3, f32)>,
}

impl LookControl {
    pub fn look_at(&mut self, point: DVec3, yaw_speed: f32) {
        self.target = Some((point, yaw_speed));
    }

    pub fn tick(&mut self, eye: DVec3, body_yaw: f32, head_yaw: &mut f32) {
        match self.target.take() {
            Some((point, speed)) => {
                let delta = point - eye;
                if delta.x * delta.x + delta.z * delta.z > 1.0e-7 {
                    *head_yaw = rotate_towards(*head_yaw, yaw_toward(delta), speed);
                }
            }
            None => *head_yaw = rotate_towards(*head_yaw, body_yaw, LOOK_IDLE_HEAD_TURN),
        }
    }
}

/// Follows a walk path by feeding its waypoints to the move control.
#[derive(Debug, Default)]
pub struct PathFollower {
    path: WalkNavigation,
    index: usize,
    active: bool,
    speed_multiplier: f32,
    stuck_ticks: u32,
    stuck_from: DVec3,
}

impl PathFollower {
    /// Follow `path` (swapped in; `path` receives the old buffer).
    pub fn install(&mut self, path: &mut WalkNavigation, speed_multiplier: f32, feet: DVec3) {
        std::mem::swap(&mut self.path, path);
        // The first waypoint is the start position.
        self.index = 1;
        self.active = true;
        self.speed_multiplier = speed_multiplier;
        self.stuck_ticks = 0;
        self.stuck_from = feet;
    }

    /// Drop the path and stop moving.
    pub fn stop(&mut self, control: &mut MoveControl) {
        self.active = false;
        self.path.clear();
        control.wanted = None;
    }

    pub fn is_done(&self) -> bool {
        !self.active || self.index >= self.path.waypoints.len()
    }

    /// The index of the waypoint being walked to and every waypoint of the
    /// path, while one is followed.
    pub fn current(&self) -> Option<(usize, &[WalkWaypoint])> {
        (!self.is_done()).then_some((self.index, self.path.waypoints.as_slice()))
    }

    /// Count the waypoint being walked to as reached.
    pub fn skip_waypoint(&mut self) {
        self.index += 1;
    }

    /// One tick of path following. `door_hold`: a door this door breaker
    /// may break stands across the path at the current waypoint; the stuck
    /// check then keeps the path, as the native follower does.
    pub fn tick(&mut self, body: &Body, control: &mut MoveControl, door_hold: bool) {
        if self.is_done() {
            self.active = false;
            return;
        }
        let reach = if body.width > PATH_WAYPOINT_REACH {
            body.width * 0.5
        } else {
            PATH_WAYPOINT_REACH - body.width * 0.5
        };
        let node = self.path.waypoints[self.index].feet;
        if (body.feet.x - node.x).abs() < reach
            && (body.feet.z - node.z).abs() < reach
            && (body.feet.y - node.y).abs() < PATH_WAYPOINT_REACH_Y
        {
            self.index += 1;
        }
        if self.index >= self.path.waypoints.len() {
            self.active = false;
            return;
        }
        let last = self.index == self.path.waypoints.len() - 1;
        control.set_wanted(
            self.path.waypoints[self.index].feet,
            self.speed_multiplier,
            last,
        );

        self.stuck_ticks += 1;
        if self.stuck_ticks >= PATH_STUCK_CHECK_TICKS {
            let speed = body.movement * self.speed_multiplier;
            let speed = if speed >= 1.0 { speed } else { speed * speed };
            let min_travel =
                f64::from(speed * PATH_STUCK_CHECK_TICKS as f32 * PATH_STUCK_DISTANCE_FACTOR);
            if !door_hold && body.feet.distance_squared(self.stuck_from) < min_travel * min_travel {
                self.stop(control);
                return;
            }
            self.stuck_ticks = 0;
            self.stuck_from = body.feet;
        }
    }
}

/// The travel step: jump, then accelerate along the body yaw by the move
/// control's speed. On the ground the acceleration is
/// `speed × (0.546 / (friction × 0.91))³` times the forward input (which
/// equals the speed); in the air the friction factor is 1; in liquid the
/// underwater speed replaces it. Returns the new velocity.
#[allow(clippy::too_many_arguments)]
pub fn travel(
    body: &Body,
    control: &mut MoveControl,
    jump: bool,
    jump_power: f32,
    yaw: f32,
    block_friction: f64,
    velocity: DVec3,
) -> DVec3 {
    use crate::entity::mob::constants::LIVING_FRICTION_FACTOR;
    let mut velocity = velocity;
    if jump {
        if body.liquid.is_some() {
            velocity.y += LIQUID_JUMP_IMPULSE;
        } else if body.on_ground && control.jump_cooldown == 0 {
            velocity.y = velocity.y.max(f64::from(jump_power));
            control.jump_cooldown = JUMP_COOLDOWN_TICKS;
        }
    }
    control.jump_cooldown = control.jump_cooldown.saturating_sub(1);

    let speed = f64::from(control.speed);
    if speed == 0.0 {
        return velocity;
    }
    let amount = if body.liquid.is_some() {
        f64::from(WATER_MOVE_SPEED)
    } else if body.on_ground {
        let friction = block_friction * LIVING_FRICTION_FACTOR;
        speed * (NORMAL_GROUND_FRICTION / friction).powi(3)
    } else {
        speed
    };
    // The forward input is the speed itself, normalised when above 1.
    let input = speed.abs().min(1.0) * speed.signum();
    velocity + forward(yaw) * (amount * input)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaw_faces_bedrock_axes() {
        assert!((yaw_toward(DVec3::Z) - 0.0).abs() < 1e-4);
        assert!((wrap_degrees(yaw_toward(DVec3::NEG_X)) - 90.0).abs() < 1e-4);
        assert!((wrap_degrees(yaw_toward(DVec3::X)) + 90.0).abs() < 1e-4);
        let f = forward(90.0);
        assert!((f.x + 1.0).abs() < 1e-9 && f.z.abs() < 1e-9);
    }

    #[test]
    fn rotation_is_limited_and_wraps() {
        assert_eq!(rotate_towards(170.0, -170.0, 30.0), -170.0);
        assert_eq!(rotate_towards(0.0, 90.0, 30.0), 30.0);
        assert_eq!(rotate_towards(0.0, -90.0, 10.0), -10.0);
    }

    fn body() -> Body {
        Body {
            feet: DVec3::ZERO,
            width: 0.9,
            height: 1.3,
            step_height: 0.5625,
            on_ground: true,
            liquid: None,
            movement: 0.25,
        }
    }

    #[test]
    fn speed_ramps_by_a_tenth_of_the_target_and_stops_on_arrival() {
        let mut control = MoveControl::default();
        control.set_wanted(DVec3::new(0.0, 0.0, 10.0), 0.8, false);
        let mut yaw = 0.0;
        let mut speeds = Vec::new();
        for _ in 0..12 {
            control.tick(&body(), &mut yaw, 30.0);
            speeds.push(control.speed);
        }
        assert!((speeds[0] - 0.02).abs() < 1e-6, "{speeds:?}");
        assert!((speeds[4] - 0.1).abs() < 1e-6, "{speeds:?}");
        assert!((speeds[11] - 0.2).abs() < 1e-6, "{speeds:?}");
        control.set_wanted(DVec3::new(0.0, 0.0, 0.2), 0.8, true);
        assert!(!control.tick(&body(), &mut yaw, 30.0));
        assert_eq!(control.wanted, None);
        assert_eq!(control.speed, 0.0);
    }

    #[test]
    fn jump_requested_for_a_close_higher_waypoint_only() {
        let mut control = MoveControl::default();
        let mut yaw = 0.0;
        control.set_wanted(DVec3::new(0.0, 1.0, 0.9), 1.0, false);
        assert!(control.tick(&body(), &mut yaw, 30.0));
        control.set_wanted(DVec3::new(0.0, 0.5, 0.9), 1.0, false);
        assert!(!control.tick(&body(), &mut yaw, 30.0), "a slab is stepped");
        control.set_wanted(DVec3::new(0.0, 1.0, 3.0), 1.0, false);
        assert!(
            !control.tick(&body(), &mut yaw, 30.0),
            "too far to jump yet"
        );
    }

    #[test]
    fn ground_acceleration_on_normal_blocks_is_speed_squared() {
        let mut control = MoveControl {
            speed: 0.2,
            ..MoveControl::default()
        };
        let v = travel(&body(), &mut control, false, 0.42, 0.0, 0.6, DVec3::ZERO);
        assert!((v.z - 0.04).abs() < 1e-6, "{v}");
        assert!(v.x.abs() < 1e-12);
    }
}
