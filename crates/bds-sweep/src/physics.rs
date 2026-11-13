//! Player movement simulated the way a Bedrock client does it, in `f32`, so
//! the positions the scripted player reports agree with BDS 1.26.51's
//! server-authoritative simulation. BDS replays every `PlayerAuthInput`
//! (move vector, rotation, input flags) and corrects a client whose
//! position strays further than `player-position-acceptance-threshold`
//! (0.5 blocks by default) with `CorrectPlayerMovePrediction`.
//!
//! One tick, on land: friction and acceleration from the block under the
//! feet (`0.91 × friction` on the ground, where the input accelerates by
//! `speed × 0.16277136 / friction³`; 0.02 in the air, 0.026 sprinting), the
//! move vector × 0.98 turned by the yaw, the jump (0.42 up, 0.2 forward
//! when sprinting, then a 10-tick delay while the key stays down), the
//! sneak's edge avoidance, collisions on y, then x, then z with a 0.5625
//! step, the landing (slime and beds bounce), and gravity
//! `vy = (vy - 0.08) × 0.98` with the friction on x and z. Water and elytra
//! gliding have their own branches. The constants were checked against BDS
//! with this client: plain walking, sprinting, jumping and falling run
//! without a correction (`tools/bds-sweep/README.md`).
//!
//! Positions here are the feet; the wire position is the feet plus
//! [`Pose::network_offset`].

use anyhow::Result;
use std::sync::LazyLock;

/// Gravity per tick.
pub const GRAVITY: f32 = 0.08;
/// Gravity per tick under slow falling.
pub const SLOW_FALLING_GRAVITY: f32 = 0.01;
/// Vertical drag after gravity.
const VERTICAL_DRAG: f32 = 0.98;
/// Horizontal friction in the air (times the block's friction on ground).
const AIR_FRICTION: f32 = 0.91;
/// Input acceleration on the ground is `speed × GROUND_FACTOR / f³`, `f`
/// the ground friction (0.546 on ordinary blocks, where it is the speed).
const GROUND_FACTOR: f32 = 0.162_771_36;
/// Input acceleration in the air, and while sprinting.
const AIR_SPEED: f32 = 0.02;
const AIR_SPEED_SPRINTING: f32 = 0.026;
/// Jump velocity, and per jump boost level.
const JUMP_VELOCITY: f32 = 0.42;
const JUMP_BOOST_STEP: f32 = 0.1;
/// Forward push of a sprinting jump.
const SPRINT_JUMP: f32 = 0.2;
/// Ticks before a held jump key jumps again.
const JUMP_DELAY: u8 = 10;
/// Highest step the player walks up.
const STEP_HEIGHT: f32 = 0.5625;
/// The move vector is scaled by this before it accelerates the player.
const IMPULSE_SCALE: f32 = 0.98;
/// Largest move-vector component while sneaking.
const SNEAK_IMPULSE: f32 = 0.3;
/// Default `minecraft:movement`, and its sprinting value (×1.3).
pub const WALK_SPEED: f32 = 0.1;
pub const SPRINT_MULTIPLIER: f32 = 1.3;
/// Penetrations this small count as touching.
const EPSILON: f32 = 1.0e-7;
/// Water: input acceleration, drag, sink rate, jump push.
const WATER_SPEED: f32 = 0.02;
const WATER_DRAG: f32 = 0.8;
const WATER_DRAG_SPRINTING: f32 = 0.9;
const WATER_SINK: f32 = 0.005;
const WATER_JUMP: f32 = 0.04;

/// An axis-aligned box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aabb {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

impl Aabb {
    /// The box of a body `width` wide and `height` tall standing at `feet`.
    pub fn from_feet(feet: [f32; 3], width: f32, height: f32) -> Self {
        let half = width / 2.0;
        Self {
            min: [feet[0] - half, feet[1], feet[2] - half],
            max: [feet[0] + half, feet[1] + height, feet[2] + half],
        }
    }

    pub fn offset(&self, d: [f32; 3]) -> Self {
        Self {
            min: [self.min[0] + d[0], self.min[1] + d[1], self.min[2] + d[2]],
            max: [self.max[0] + d[0], self.max[1] + d[1], self.max[2] + d[2]],
        }
    }

    /// The box stretched towards `d` (the space a move of `d` sweeps).
    pub fn expand(&self, d: [f32; 3]) -> Self {
        let mut out = *self;
        for (axis, d) in d.into_iter().enumerate() {
            if d < 0.0 {
                out.min[axis] += d;
            } else {
                out.max[axis] += d;
            }
        }
        out
    }

    /// The box grown by `d` on every side (shrunk for negative `d`).
    pub fn inflate(&self, d: [f32; 3]) -> Self {
        Self {
            min: [self.min[0] - d[0], self.min[1] - d[1], self.min[2] - d[2]],
            max: [self.max[0] + d[0], self.max[1] + d[1], self.max[2] + d[2]],
        }
    }

    pub fn intersects(&self, other: &Aabb) -> bool {
        (0..3).all(|axis| self.min[axis] < other.max[axis] && self.max[axis] > other.min[axis])
    }

    /// The blocks the box covers, inclusive.
    pub fn block_range(&self) -> ([i32; 3], [i32; 3]) {
        (
            self.min.map(|v| v.floor() as i32),
            self.max.map(|v| v.floor() as i32),
        )
    }

    /// The feet of the body this box is.
    fn feet(&self) -> [f32; 3] {
        [
            (self.min[0] + self.max[0]) * 0.5,
            self.min[1],
            (self.min[2] + self.max[2]) * 0.5,
        ]
    }

    /// `velocity` on `axis` clipped so this box does not move into `block`.
    fn clip(&self, block: &Aabb, axis: usize, velocity: f32) -> f32 {
        if velocity == 0.0 {
            return 0.0;
        }
        for other in 0..3 {
            if other == axis {
                continue;
            }
            let below = self.max[other] - block.min[other];
            let above = block.max[other] - self.min[other];
            if below <= EPSILON || above <= EPSILON {
                return velocity;
            }
        }
        if velocity > 0.0 {
            let gap = block.min[axis] - self.max[axis];
            if gap >= -EPSILON && gap < velocity {
                return gap.max(0.0);
            }
        } else {
            let gap = block.max[axis] - self.min[axis];
            if gap <= EPSILON && gap > velocity {
                return gap.min(0.0);
            }
        }
        velocity
    }
}

/// The 65536-entry sine table Minecraft's movement trigonometry reads.
static SIN_TABLE: LazyLock<Vec<f32>> = LazyLock::new(|| {
    (0..65536)
        .map(|i| (f64::from(i) * std::f64::consts::PI * 2.0 / 65536.0).sin() as f32)
        .collect()
});

/// `sin` from the table: entry `(r × 10430.378) & 65535`.
fn table_sin(radians: f32) -> f32 {
    SIN_TABLE[((radians * 10430.378) as i32 & 65535) as usize]
}

/// `cos` from the table: a quarter turn (16384 entries) on.
fn table_cos(radians: f32) -> f32 {
    SIN_TABLE[((radians * 10430.378 + 16384.0) as i32 & 65535) as usize]
}

/// The body's shape, which sets its box and the height of the wire
/// position above the feet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pose {
    Standing,
    Sneaking,
    /// Swimming, gliding and crawling: 0.6 tall.
    Low,
}

impl Pose {
    pub fn height(self) -> f32 {
        match self {
            Pose::Standing => 1.8,
            Pose::Sneaking => 1.49,
            Pose::Low => 0.6,
        }
    }

    /// The wire position is the feet plus this, in every pose (BDS's
    /// teleport `MovePlayer` puts the eyes 1.62001 above the feet, and its
    /// corrections of a sneaking player do the same).
    pub fn network_offset(self) -> f32 {
        1.620_01
    }
}

/// What the player's keys and mouse do this tick.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Controls {
    /// Forward (W positive) and strafe (A positive), each in -1..=1.
    pub forward: f32,
    pub strafe: f32,
    /// Degrees.
    pub yaw: f32,
    pub pitch: f32,
    pub jump: bool,
    pub sneak: bool,
    pub sprint: bool,
}

/// The block under the feet as movement sees it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ground {
    pub friction: f32,
    pub kind: GroundKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroundKind {
    Plain,
    Slime,
    Bed,
    Air,
}

/// What movement needs from the world.
pub trait Terrain {
    /// Collision boxes that may touch `area`.
    fn collisions(&self, area: &Aabb) -> Result<Vec<Aabb>>;
    /// Every block of `area` is loaded.
    fn loaded(&self, area: &Aabb) -> bool;
    /// The block at `pos`.
    fn ground(&self, pos: [i32; 3]) -> Ground;
    /// Water in any block `area` touches.
    fn water(&self, area: &Aabb) -> bool;
    /// How a block the body is in scales its move, if it does (powder snow
    /// at the feet, cobwebs anywhere in the box).
    fn stuck(&self, body: &Aabb) -> Option<[f32; 3]> {
        let _ = body;
        None
    }
}

/// The simulated player.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Body {
    pub feet: [f32; 3],
    pub velocity: [f32; 3],
    pub on_ground: bool,
    pub jump_delay: u8,
    pub sprinting: bool,
    pub sneaking: bool,
    pub swimming: bool,
    pub gliding: bool,
    pub in_water: bool,
    /// `minecraft:movement` as last set (0.1, 0.13 sprinting).
    pub movement_speed: f32,
    /// The last tick moved as a sprinter.
    pub moved_sprinting: bool,
    pub jump_boost: u8,
    pub slow_falling: bool,
    /// The last tick's move collided horizontally / vertically.
    pub horizontal_collision: bool,
    pub vertical_collision: bool,
}

impl Body {
    pub fn at(feet: [f32; 3]) -> Self {
        Self {
            feet,
            velocity: [0.0; 3],
            on_ground: false,
            jump_delay: 0,
            sprinting: false,
            sneaking: false,
            swimming: false,
            gliding: false,
            in_water: false,
            movement_speed: WALK_SPEED,
            moved_sprinting: false,
            jump_boost: 0,
            slow_falling: false,
            horizontal_collision: false,
            vertical_collision: false,
        }
    }

    pub fn pose(&self) -> Pose {
        if self.swimming || self.gliding {
            Pose::Low
        } else if self.sneaking {
            Pose::Sneaking
        } else {
            Pose::Standing
        }
    }

    pub fn aabb(&self) -> Aabb {
        Aabb::from_feet(self.feet, 0.6, self.pose().height())
    }

    /// The wire position.
    pub fn network_position(&self) -> [f32; 3] {
        [
            self.feet[0],
            self.feet[1] + self.pose().network_offset(),
            self.feet[2],
        ]
    }

    /// Feet from a wire position in the current pose.
    pub fn feet_from_network(&self, position: [f32; 3]) -> [f32; 3] {
        [
            position[0],
            position[1] - self.pose().network_offset(),
            position[2],
        ]
    }

    /// The box BDS checks for water: the body's box without its top 0.4 and
    /// bottom 0.4 (but at least a slice), shrunk by 0.001.
    fn water_box(&self) -> Aabb {
        let bb = self.aabb();
        let shrink_y = ((bb.max[1] - bb.min[1]) / 2.0 - 0.001).min(0.4);
        bb.inflate([-0.001, -shrink_y, -0.001])
    }
}

/// What a tick changed that the input flags report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Changes {
    pub started_sprinting: bool,
    pub stopped_sprinting: bool,
    pub started_sneaking: bool,
    pub stopped_sneaking: bool,
    pub started_swimming: bool,
    pub stopped_swimming: bool,
    pub stopped_gliding: bool,
    pub jumped: bool,
    /// The chunks around the body are not loaded: it stayed put.
    pub frozen: bool,
}

/// Apply this tick's controls to the body's movement state (sprint, sneak,
/// swim) like the client does before it moves. A sprint (and the swim it
/// carries) starts before this tick's move but stops after it: BDS still
/// moves the player as a sprinter in the tick its `StopSprinting` arrives.
/// Returns whether the sprint stops after the move.
pub fn update_state(body: &mut Body, controls: &Controls, changes: &mut Changes) -> bool {
    if controls.sneak != body.sneaking {
        body.sneaking = controls.sneak;
        if controls.sneak {
            changes.started_sneaking = true;
        } else {
            changes.stopped_sneaking = true;
        }
    }
    // The sprint key starts a sprint; it lasts while the player keeps moving
    // forward without sneaking (BDS answers a stop sent while still moving
    // forward with the unchanged sprint).
    let can_sprint = controls.forward > 0.0 && !controls.sneak;
    let want_sprint = can_sprint && (controls.sprint || body.sprinting);
    let mut stop = false;
    if want_sprint && !body.sprinting {
        body.sprinting = true;
        body.movement_speed = WALK_SPEED * SPRINT_MULTIPLIER;
        changes.started_sprinting = true;
    } else if !want_sprint && body.sprinting {
        changes.stopped_sprinting = true;
        stop = true;
    }
    let want_swim = want_sprint && body.in_water;
    if want_swim && !body.swimming {
        body.swimming = true;
        changes.started_swimming = true;
    } else if !want_swim && body.swimming {
        changes.stopped_swimming = true;
    }
    stop
}

/// Move the body one tick.
pub fn step(body: &mut Body, controls: &Controls, terrain: &impl Terrain) -> Result<Changes> {
    let mut changes = Changes::default();
    // The sprint the last tick moved with: water drag keeps it a tick on.
    let was_sprinting = body.moved_sprinting;
    let stop_sprint = update_state(body, controls, &mut changes);
    if !terrain.loaded(&body.aabb().inflate([1.0, 1.0, 1.0])) {
        body.velocity = [0.0; 3];
        changes.frozen = true;
        finish_sprint(body, stop_sprint, &changes);
        return Ok(changes);
    }
    if body.velocity.iter().map(|v| v * v).sum::<f32>() < 1.0e-12 {
        body.velocity = [0.0; 3];
    }
    body.in_water = terrain.water(&body.water_box());
    if body.gliding && (body.on_ground || body.in_water) {
        body.gliding = false;
        changes.stopped_gliding = true;
    }
    if body.in_water {
        water_tick(body, controls, terrain, was_sprinting)?;
    } else if body.gliding {
        glide_tick(body, controls, terrain)?;
    } else {
        land_tick(body, controls, terrain, &mut changes)?;
    }
    if !controls.jump {
        body.jump_delay = 0;
    } else if body.jump_delay > 0 {
        body.jump_delay -= 1;
    }
    finish_sprint(body, stop_sprint, &changes);
    Ok(changes)
}

/// After the move: remember the sprint it used, then end a stopping sprint
/// (and swim).
fn finish_sprint(body: &mut Body, stop: bool, changes: &Changes) {
    body.moved_sprinting = body.sprinting;
    if stop {
        body.sprinting = false;
        body.movement_speed = WALK_SPEED;
    }
    if changes.stopped_swimming {
        body.swimming = false;
    }
}

/// The move vector's acceleration at `speed`, turned by the yaw.
fn move_relative(body: &mut Body, controls: &Controls, speed: f32) {
    let limit = if body.sneaking { SNEAK_IMPULSE } else { 1.0 };
    let forward = controls.forward.clamp(-limit, limit) * IMPULSE_SCALE;
    let strafe = controls.strafe.clamp(-limit, limit) * IMPULSE_SCALE;
    let force = forward * forward + strafe * strafe;
    if force < 1.0e-4 {
        return;
    }
    let scale = speed / force.sqrt().max(1.0);
    let (forward, strafe) = (forward * scale, strafe * scale);
    let yaw = controls.yaw * std::f32::consts::PI / 180.0;
    let (sin, cos) = (table_sin(yaw), table_cos(yaw));
    body.velocity[0] += strafe * cos - forward * sin;
    body.velocity[2] += forward * cos + strafe * sin;
}

/// The block under the feet that sets friction (half a block down).
fn friction_block(body: &Body, terrain: &impl Terrain) -> Ground {
    terrain.ground([
        body.feet[0].floor() as i32,
        (body.feet[1] - 0.5).floor() as i32,
        body.feet[2].floor() as i32,
    ])
}

/// The block the feet rest on after a move.
fn landing_block(body: &Body, terrain: &impl Terrain) -> Ground {
    let x = body.feet[0].floor() as i32;
    let z = body.feet[2].floor() as i32;
    let ground = terrain.ground([x, (body.feet[1] - 0.2).floor() as i32, z]);
    if ground.kind == GroundKind::Air {
        terrain.ground([x, body.feet[1].floor() as i32 - 1, z])
    } else {
        ground
    }
}

fn land_tick(
    body: &mut Body,
    controls: &Controls,
    terrain: &impl Terrain,
    changes: &mut Changes,
) -> Result<()> {
    let mut friction = AIR_FRICTION;
    let mut speed = if body.sprinting {
        AIR_SPEED_SPRINTING
    } else {
        AIR_SPEED
    };
    if body.on_ground {
        friction *= friction_block(body, terrain).friction;
        speed = body.movement_speed * (GROUND_FACTOR / (friction * friction * friction));
    }
    move_relative(body, controls, speed);
    if controls.jump && body.on_ground && body.jump_delay == 0 {
        body.velocity[1] =
            body.velocity[1].max(JUMP_VELOCITY + JUMP_BOOST_STEP * f32::from(body.jump_boost));
        body.jump_delay = JUMP_DELAY;
        if body.sprinting {
            let yaw = controls.yaw * 0.017_453_292;
            body.velocity[0] -= table_sin(yaw) * SPRINT_JUMP;
            body.velocity[2] += table_cos(yaw) * SPRINT_JUMP;
        }
        changes.jumped = true;
    }
    if body.sneaking && body.on_ground && body.velocity[1] <= 0.0 {
        avoid_edges(body, terrain)?;
    }
    let before = body.velocity;
    let was_on_ground = body.on_ground;
    let stuck = terrain.stuck(&body.aabb());
    if let Some(scale) = stuck {
        // A web or powder snow scales this move and stops the body after it.
        for (velocity, scale) in body.velocity.iter_mut().zip(scale) {
            *velocity *= scale;
        }
    }
    move_and_collide(body, terrain)?;
    if stuck.is_some() {
        body.velocity = [0.0; 3];
    }
    let ground = landing_block(body, terrain);
    if body.on_ground && !body.sneaking && ground.kind == GroundKind::Slime {
        let vertical = body.velocity[1].abs();
        if vertical < 0.1 {
            let slow = 0.4 + vertical * 0.2;
            body.velocity[0] *= slow;
            body.velocity[2] *= slow;
        }
    }
    if body.vertical_collision {
        body.velocity[1] = if !was_on_ground && before[1] < 0.0 && !controls.sneak {
            match ground.kind {
                GroundKind::Slime => {
                    let bounce = -before[1];
                    if bounce.abs() < 1.0e-4 { 0.0 } else { bounce }
                }
                GroundKind::Bed => (-0.66 * before[1]).min(1.0),
                _ => 0.0,
            }
        } else {
            0.0
        };
    }
    let gravity = if body.slow_falling {
        SLOW_FALLING_GRAVITY
    } else {
        GRAVITY
    };
    body.velocity[1] = (body.velocity[1] - gravity) * VERTICAL_DRAG;
    body.velocity[0] *= friction;
    body.velocity[2] *= friction;
    Ok(())
}

/// One tick in water: the swimmer's pitch, the jump's lift, 0.02
/// acceleration, drag 0.8 (0.9 for a sprinter, still in the tick its sprint
/// stops) and a 0.005 sink unless sprinting.
fn water_tick(
    body: &mut Body,
    controls: &Controls,
    terrain: &impl Terrain,
    was_sprinting: bool,
) -> Result<()> {
    if body.swimming {
        // A swimmer heads where it looks: the vertical velocity moves
        // towards the look's y by 8.5 % a tick looking down (below -0.2),
        // 6 % otherwise, the latter only under water or jumping.
        let look_y = -table_sin(controls.pitch * std::f32::consts::PI / 180.0);
        let rate = if look_y < -0.2 { 0.085 } else { 0.06 };
        let head = [
            body.feet[0].floor() as i32,
            (body.feet[1] + 0.9).floor() as i32,
            body.feet[2].floor() as i32,
        ];
        let under = Aabb {
            min: head.map(|v| v as f32),
            max: head.map(|v| v as f32 + 1.0),
        };
        if look_y <= 0.0 || controls.jump || terrain.water(&under.inflate([-0.01; 3])) {
            body.velocity[1] += (look_y - body.velocity[1]) * rate;
        }
    }
    if controls.jump {
        body.velocity[1] += WATER_JUMP;
    }
    move_relative(body, controls, WATER_SPEED);
    let y = body.feet[1];
    move_and_collide(body, terrain)?;
    let drag = if body.sprinting || was_sprinting {
        WATER_DRAG_SPRINTING
    } else {
        WATER_DRAG
    };
    body.velocity[0] *= drag;
    body.velocity[1] *= WATER_DRAG;
    body.velocity[2] *= drag;
    if !body.sprinting {
        body.velocity[1] -= WATER_SINK;
    }
    if body.horizontal_collision {
        let probe = body.aabb().offset([
            body.velocity[0],
            body.velocity[1] + 0.6 - body.feet[1] + y,
            body.velocity[2],
        ]);
        if terrain.collisions(&probe)?.is_empty() && !terrain.water(&probe) {
            body.velocity[1] = 0.3;
        }
    }
    Ok(())
}

fn glide_tick(body: &mut Body, controls: &Controls, terrain: &impl Terrain) -> Result<()> {
    let radians = std::f32::consts::PI / 180.0;
    let (yaw, pitch) = (controls.yaw * radians, controls.pitch * radians);
    let pitch_cos = table_cos(pitch);
    let pitch_sin = table_sin(pitch);
    let look = [
        -table_sin(yaw) * pitch_cos,
        -pitch_sin,
        table_cos(yaw) * pitch_cos,
    ];
    let v = &mut body.velocity;
    let horizontal_speed = (v[0] * v[0] + v[2] * v[2]).sqrt();
    let look_horizontal = pitch_cos;
    let lift = pitch_cos * pitch_cos;
    v[1] += -GRAVITY + lift * 0.06;
    if v[1] < 0.0 && look_horizontal > 0.0 {
        let accel = v[1] * -0.1 * lift;
        v[1] += accel;
        v[0] += look[0] * accel / look_horizontal;
        v[2] += look[2] * accel / look_horizontal;
    }
    if pitch < 0.0 && look_horizontal > 0.0 {
        let accel = horizontal_speed * -pitch_sin * 0.04;
        v[1] += accel * 3.2;
        v[0] -= look[0] * accel / look_horizontal;
        v[2] -= look[2] * accel / look_horizontal;
    }
    if look_horizontal > 0.0 {
        v[0] += (look[0] / look_horizontal * horizontal_speed - v[0]) * 0.1;
        v[2] += (look[2] / look_horizontal * horizontal_speed - v[2]) * 0.1;
    }
    v[0] *= 0.99;
    v[1] *= 0.98;
    v[2] *= 0.99;
    let _ = controls;
    move_and_collide(body, terrain)
}

/// Sneaking on the ground: shorten the move until the body would still
/// stand on something (0.025 in from its sides, 0.05 at a time).
fn avoid_edges(body: &mut Body, terrain: &impl Terrain) -> Result<()> {
    const INSET: f32 = 0.025;
    const STEP: f32 = 0.05;
    let bb = body.aabb().inflate([-INSET, 0.0, -INSET]);
    let drop = -STEP_HEIGHT * 1.01;
    let unsupported = |dx: f32, dz: f32| -> Result<bool> {
        let probe = bb.offset([dx, drop, dz]);
        Ok(terrain.collisions(&probe)?.is_empty())
    };
    let shorten = |v: f32| {
        if v.abs() < STEP {
            0.0
        } else if v > 0.0 {
            v - STEP
        } else {
            v + STEP
        }
    };
    let [mut dx, _, mut dz] = body.velocity;
    while dx != 0.0 && unsupported(dx, 0.0)? {
        dx = shorten(dx);
    }
    while dz != 0.0 && unsupported(0.0, dz)? {
        dz = shorten(dz);
    }
    while dx != 0.0 && dz != 0.0 && unsupported(dx, dz)? {
        dx = shorten(dx);
        dz = shorten(dz);
    }
    body.velocity[0] = dx;
    body.velocity[2] = dz;
    Ok(())
}

/// Clip `velocity` against `boxes` on y, then x, then z from `bb`; returns
/// the moved box and the clipped velocity.
fn clip_all(bb: Aabb, velocity: [f32; 3], boxes: &[Aabb]) -> (Aabb, [f32; 3]) {
    let mut bb = bb;
    let mut out = [0.0f32; 3];
    for axis in [1, 0, 2] {
        let mut v = velocity[axis];
        for block in boxes {
            v = bb.clip(block, axis, v);
        }
        let mut d = [0.0; 3];
        d[axis] = v;
        bb = bb.offset(d);
        out[axis] = v;
    }
    (bb, out)
}

/// Move the body by its velocity, colliding with the world (and stepping up
/// to [`STEP_HEIGHT`] on the ground), and update its collision state.
fn move_and_collide(body: &mut Body, terrain: &impl Terrain) -> Result<()> {
    let velocity = body.velocity;
    let start = body.aabb();
    let boxes = terrain.collisions(&start.expand(velocity).inflate([1.0e-3; 3]))?;
    let (mut bb, mut moved) = clip_all(start, velocity, &boxes);
    let hit = |axis: usize, moved: &[f32; 3]| (velocity[axis] - moved[axis]).abs() >= 1.0e-5;
    let grounded = body.on_ground || (hit(1, &moved) && velocity[1] < 0.0);
    if grounded && (hit(0, &moved) || hit(2, &moved)) {
        let reach = start.expand([velocity[0], STEP_HEIGHT, velocity[2]]);
        let step_boxes: Vec<Aabb> = terrain
            .collisions(&reach.inflate([1.0e-3; 3]))?
            .into_iter()
            .filter(|b| b.min[1] < start.max[1])
            .collect();
        let (up_bb, up) = clip_all(start, [0.0, STEP_HEIGHT, 0.0], &step_boxes);
        let (across_bb, across) = clip_all(up_bb, [velocity[0], 0.0, velocity[2]], &step_boxes);
        let (down_bb, down) = clip_all(across_bb, [0.0, -up[1], 0.0], &step_boxes);
        let stepped = [across[0], up[1] + down[1], across[2]];
        let blocked = terrain
            .collisions(&down_bb)?
            .iter()
            .any(|b| b.intersects(&down_bb));
        if !blocked
            && moved[0] * moved[0] + moved[2] * moved[2]
                < stepped[0] * stepped[0] + stepped[2] * stepped[2]
        {
            bb = down_bb;
            moved = stepped;
        }
    }
    body.feet = bb.feet();
    let x_hit = hit(0, &moved);
    let y_hit = hit(1, &moved);
    let z_hit = hit(2, &moved);
    body.on_ground =
        (y_hit && velocity[1] < 0.0) || (body.on_ground && !y_hit && velocity[1].abs() <= 1.0e-5);
    body.horizontal_collision = x_hit || z_hit;
    body.vertical_collision = y_hit;
    body.velocity = moved;
    if x_hit {
        body.velocity[0] = 0.0;
    }
    if z_hit {
        body.velocity[2] = 0.0;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A flat floor whose top is at `top`, of one block kind, and water
    /// from `water` up (if any).
    struct Flat {
        top: f32,
        kind: GroundKind,
        friction: f32,
        water: Option<f32>,
    }

    impl Flat {
        fn stone(top: f32) -> Self {
            Self {
                top,
                kind: GroundKind::Plain,
                friction: 0.6,
                water: None,
            }
        }
    }

    impl Terrain for Flat {
        fn collisions(&self, area: &Aabb) -> Result<Vec<Aabb>> {
            let floor = Aabb {
                min: [-1000.0, self.top - 1.0, -1000.0],
                max: [1000.0, self.top, 1000.0],
            };
            Ok(if floor.intersects(area) {
                vec![floor]
            } else {
                vec![]
            })
        }
        fn loaded(&self, _: &Aabb) -> bool {
            true
        }
        fn ground(&self, pos: [i32; 3]) -> Ground {
            if pos[1] == self.top as i32 - 1 {
                Ground {
                    friction: self.friction,
                    kind: self.kind,
                }
            } else {
                Ground {
                    friction: 0.6,
                    kind: GroundKind::Air,
                }
            }
        }
        fn water(&self, area: &Aabb) -> bool {
            self.water
                .is_some_and(|w| area.min[1] < w + 100.0 && area.max[1] > w)
        }
    }

    fn standing(top: f32) -> Body {
        let mut body = Body::at([0.5, top, 0.5]);
        body.on_ground = true;
        body
    }

    fn walk(forward: f32) -> Controls {
        Controls {
            forward,
            ..Default::default()
        }
    }

    #[test]
    fn standing_still_reports_the_ground_velocity() {
        let terrain = Flat::stone(-60.0);
        let mut body = standing(-60.0);
        // The first tick starts at rest, so it does not touch the floor yet.
        step(&mut body, &Controls::default(), &terrain).unwrap();
        for _ in 0..5 {
            step(&mut body, &Controls::default(), &terrain).unwrap();
            assert_eq!(body.feet, [0.5, -60.0, 0.5]);
            assert!(body.on_ground && body.vertical_collision);
            assert_eq!(body.velocity, [0.0, -0.0784, 0.0]);
        }
        assert_eq!(body.network_position()[1], -60.0 + 1.620_01);
    }

    #[test]
    fn walking_and_sprinting_reach_their_vanilla_speeds() {
        let terrain = Flat::stone(-60.0);
        for (sprint, expected) in [(false, 0.215_85), (true, 0.280_6)] {
            let mut body = standing(-60.0);
            let controls = Controls {
                sprint,
                ..walk(1.0)
            };
            let mut last = body.feet[2];
            let mut per_tick = 0.0;
            for _ in 0..60 {
                step(&mut body, &controls, &terrain).unwrap();
                per_tick = body.feet[2] - last;
                last = body.feet[2];
            }
            assert!((per_tick - expected).abs() < 2.0e-4, "{sprint}: {per_tick}");
            assert_eq!(body.feet[0], 0.5, "yaw 0 walks along +z");
        }
    }

    #[test]
    fn a_jump_peaks_at_its_vanilla_height_and_lands_after_twelve_ticks() {
        let terrain = Flat::stone(0.0);
        let mut body = standing(0.0);
        let mut peak = 0.0f32;
        let mut landed = None;
        for tick in 1..=20 {
            let controls = Controls {
                jump: tick == 1,
                ..Default::default()
            };
            let changes = step(&mut body, &controls, &terrain).unwrap();
            assert_eq!(changes.jumped, tick == 1);
            peak = peak.max(body.feet[1]);
            if tick > 1 && body.on_ground && landed.is_none() {
                landed = Some(tick);
            }
        }
        assert!((peak - 1.2522).abs() < 1.0e-3, "{peak}");
        assert_eq!(landed, Some(12));
        assert_eq!(body.feet[1], 0.0);
    }

    #[test]
    fn falling_follows_gravity_and_drag() {
        let terrain = Flat::stone(-60.0);
        let mut body = Body::at([0.5, -50.0, 0.5]);
        step(&mut body, &Controls::default(), &terrain).unwrap();
        assert_eq!(body.feet[1], -50.0);
        assert!((body.velocity[1] + 0.0784).abs() < 1e-7);
        step(&mut body, &Controls::default(), &terrain).unwrap();
        assert!((body.feet[1] + 50.0784).abs() < 1e-5);
        let mut ticks = 2;
        while !body.on_ground {
            step(&mut body, &Controls::default(), &terrain).unwrap();
            ticks += 1;
        }
        assert_eq!(body.feet[1], -60.0);
        assert_eq!(ticks, 18, "a 10-block fall lands on its 18th tick");
    }

    #[test]
    fn slime_bounces_a_landing_and_sneaking_does_not() {
        for (sneak, bounces) in [(false, true), (true, false)] {
            let terrain = Flat {
                kind: GroundKind::Slime,
                friction: 0.8,
                ..Flat::stone(0.0)
            };
            let mut body = Body::at([0.5, 3.0, 0.5]);
            body.sneaking = sneak;
            let controls = Controls {
                sneak,
                ..Default::default()
            };
            while !body.on_ground {
                step(&mut body, &controls, &terrain).unwrap();
            }
            assert_eq!(body.velocity[1] > 0.0, bounces, "{:?}", body.velocity);
        }
    }

    #[test]
    fn sneaking_stops_at_an_edge() {
        struct Ledge;
        impl Terrain for Ledge {
            fn collisions(&self, area: &Aabb) -> Result<Vec<Aabb>> {
                let ledge = Aabb {
                    min: [-10.0, -1.0, -10.0],
                    max: [1.0, 0.0, 10.0],
                };
                Ok(if ledge.intersects(area) {
                    vec![ledge]
                } else {
                    vec![]
                })
            }
            fn loaded(&self, _: &Aabb) -> bool {
                true
            }
            fn ground(&self, _: [i32; 3]) -> Ground {
                Ground {
                    friction: 0.6,
                    kind: GroundKind::Plain,
                }
            }
            fn water(&self, _: &Aabb) -> bool {
                false
            }
        }
        let mut body = standing(0.0);
        body.sneaking = true;
        let controls = Controls {
            yaw: -90.0,
            sneak: true,
            ..walk(1.0)
        };
        for _ in 0..80 {
            step(&mut body, &controls, &Ledge).unwrap();
        }
        assert!(body.on_ground);
        assert!(body.feet[0] < 1.3 && body.feet[0] > 1.0, "{:?}", body.feet);
    }

    #[test]
    fn unloaded_terrain_freezes_the_body() {
        struct Void;
        impl Terrain for Void {
            fn collisions(&self, _: &Aabb) -> Result<Vec<Aabb>> {
                Ok(vec![])
            }
            fn loaded(&self, _: &Aabb) -> bool {
                false
            }
            fn ground(&self, _: [i32; 3]) -> Ground {
                Ground {
                    friction: 0.6,
                    kind: GroundKind::Air,
                }
            }
            fn water(&self, _: &Aabb) -> bool {
                false
            }
        }
        let mut body = Body::at([0.5, 70.0, 0.5]);
        let changes = step(&mut body, &walk(1.0), &Void).unwrap();
        assert!(changes.frozen);
        assert_eq!(body.feet, [0.5, 70.0, 0.5]);
    }

    #[test]
    fn idle_in_water_sinks_towards_the_water_terminal_speed() {
        let terrain = Flat {
            water: Some(-80.0),
            ..Flat::stone(-100.0)
        };
        let mut body = Body::at([0.5, -70.0, 0.5]);
        let mut last = body.feet[1];
        let mut per_tick = 0.0;
        for _ in 0..80 {
            step(&mut body, &Controls::default(), &terrain).unwrap();
            per_tick = body.feet[1] - last;
            last = body.feet[1];
        }
        assert!(body.in_water);
        // v' = 0.8 v - 0.005 settles at -0.025.
        assert!((per_tick + 0.025).abs() < 1e-5, "{per_tick}");
    }

    #[test]
    fn a_swimmer_heads_where_it_looks_and_keeps_the_sprint_drag_a_tick() {
        let terrain = Flat {
            water: Some(-80.0),
            ..Flat::stone(-100.0)
        };
        let mut body = Body::at([0.5, -70.0, 0.5]);
        body.in_water = true;
        let swim = Controls {
            sprint: true,
            pitch: 30.0,
            ..walk(1.0)
        };
        let changes = step(&mut body, &swim, &terrain).unwrap();
        assert!(changes.started_sprinting && changes.started_swimming);
        let mut last = body.feet[1];
        let mut per_tick = 0.0;
        for _ in 0..60 {
            step(&mut body, &swim, &terrain).unwrap();
            per_tick = body.feet[1] - last;
            last = body.feet[1];
        }
        // Looking 30 degrees down: d' = 0.8 (1 - 0.085) d - 0.0425.
        let steady = -0.0425 / (1.0 - 0.8 * 0.915);
        assert!((per_tick - steady).abs() < 1e-4, "{per_tick} vs {steady}");
        // Letting go: the sprint (and its 0.9 drag) still moves this tick.
        let changes = step(&mut body, &Controls::default(), &terrain).unwrap();
        assert!(changes.stopped_sprinting && changes.stopped_swimming);
        assert!(!body.sprinting && !body.swimming && body.moved_sprinting);
    }

    #[test]
    fn a_sprint_lasts_while_moving_forward_without_the_key() {
        let terrain = Flat::stone(0.0);
        let mut body = standing(0.0);
        let sprint = Controls {
            sprint: true,
            ..walk(1.0)
        };
        assert!(
            step(&mut body, &sprint, &terrain)
                .unwrap()
                .started_sprinting
        );
        let changes = step(&mut body, &walk(1.0), &terrain).unwrap();
        assert!(body.sprinting && !changes.stopped_sprinting);
        let changes = step(&mut body, &walk(0.0), &terrain).unwrap();
        assert!(changes.stopped_sprinting && !body.sprinting);
        assert_eq!(body.movement_speed, WALK_SPEED);
    }

    #[test]
    fn sprinting_needs_forward_input_and_no_sneak() {
        let mut body = standing(0.0);
        let mut changes = Changes::default();
        update_state(
            &mut body,
            &Controls {
                sprint: true,
                ..Default::default()
            },
            &mut changes,
        );
        assert!(!body.sprinting && !changes.started_sprinting);
        update_state(
            &mut body,
            &Controls {
                sprint: true,
                ..walk(1.0)
            },
            &mut changes,
        );
        assert!(body.sprinting && changes.started_sprinting);
        assert!((body.movement_speed - 0.13).abs() < 1e-7);
    }
}
