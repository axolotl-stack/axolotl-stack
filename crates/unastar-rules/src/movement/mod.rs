//! Player movement as Bedrock simulates it, shared by the server's movement
//! authority (`unastar`) and the scripted sweep client (`bds-sweep`) that
//! measures it on BDS 1.26.51, so both run the same `f32` arithmetic.
//!
//! BDS replays every `PlayerAuthInput` (move vector, rotation, input flags)
//! through this simulation and keeps its own copy of the player
//! ([`authority`] decides when that copy takes the client's position, is
//! pulled towards it, or corrects it). A client that reports what this
//! module computes is never corrected; `tools/bds-sweep/README.md` lists the
//! movements checked that way.
//!
//! One tick on land: friction and acceleration from the block under the feet
//! (`0.91 × friction` on the ground, where the input accelerates by the
//! speed times `(0.546 / f)³`, rounded after each factor; 0.02 in the air,
//! 0.026 sprinting), the
//! move vector × 0.98 turned by the yaw, the jump (0.42 up, 0.2 forward when
//! sprinting, then a 10-tick delay while the key stays down), the sneak's
//! edge avoidance, collisions on y, then x, then z with a 0.5625 step, the
//! landing (slime and beds bounce), and gravity `vy = (vy - 0.08) × 0.98`
//! with the friction on x and z. Water and elytra gliding have their own
//! branches.
//!
//! Positions are the feet; the wire position is the feet plus
//! [`Pose::network_offset`].

pub mod authority;
#[cfg(test)]
mod tests;

use std::sync::LazyLock;

/// Gravity per tick.
pub const GRAVITY: f32 = 0.08;
/// Gravity per tick under slow falling.
pub const SLOW_FALLING_GRAVITY: f32 = 0.01;
/// Vertical drag after gravity.
const VERTICAL_DRAG: f32 = 0.98;
/// Horizontal friction in the air (times the block's friction on ground).
const AIR_FRICTION: f32 = 0.91;
/// Friction of ordinary ground (0.6) in the air's 0.91: the reference of the
/// ground acceleration.
const BASE_GROUND_FRICTION: f32 = AIR_FRICTION * 0.6;
/// Input acceleration in the air, and while sprinting.
const AIR_SPEED: f32 = 0.02;
const AIR_SPEED_SPRINTING: f32 = 0.026;
/// Jump velocity, and per jump boost level.
const JUMP_VELOCITY: f32 = 0.42;
const JUMP_BOOST_STEP: f32 = 0.1;
/// Climbing speed on ladders and vines, and their slowest fall.
const CLIMB_SPEED: f32 = 0.2;
/// The slowest fall onto slime that bounces (just over a standing player's
/// 0.0784).
const SLIME_MIN_BOUNCE: f32 = 0.080_000_12;
/// Soul sand's friction is scaled by this for the acceleration only.
const SOUL_SAND_ACCELERATION: f32 = 1.225;
/// Levitation's pull per level each tick: a fifth of its 0.05 speed.
const LEVITATION_LIFT: f32 = 0.01;
/// A jump from honey is this much of a jump.
const HONEY_JUMP: f32 = 0.6;
/// Forward push of a sprinting jump.
const SPRINT_JUMP: f32 = 0.2;
/// Ticks before a held jump key jumps again.
pub const JUMP_DELAY: u8 = 10;
/// Highest step the player walks up.
pub const STEP_HEIGHT: f32 = 0.5625;
/// The move vector is scaled by this before it accelerates the player.
const IMPULSE_SCALE: f32 = 0.98;
/// The move vector's factor while sneaking.
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
/// Player box width and its height standing, sneaking and low (swimming,
/// gliding, crawling).
pub const WIDTH: f32 = 0.6;
/// The wire position's height above the feet, in every pose (BDS's
/// teleport `MovePlayer` puts the eyes 1.62001 above the feet, and its
/// corrections of a sneaking player do the same).
pub const NETWORK_OFFSET: f32 = 1.620_01;

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
    pub fn feet(&self) -> [f32; 3] {
        [
            (self.min[0] + self.max[0]) * 0.5,
            self.min[1],
            (self.min[2] + self.max[2]) * 0.5,
        ]
    }

    /// `velocity` on `axis` clipped so this box does not move into `block`.
    fn clip(&self, block: &Aabb, axis: usize, velocity: f32) -> f32 {
        if velocity == 0.0 {
            // Keeps the sign: BDS carries a stopped backward slide as -0.
            return velocity;
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
pub fn table_sin(radians: f32) -> f32 {
    SIN_TABLE[((radians * 10430.378) as i32 & 65535) as usize]
}

/// `cos` from the table: a quarter turn (16384 entries) on.
pub fn table_cos(radians: f32) -> f32 {
    SIN_TABLE[((radians * 10430.378 + 16384.0) as i32 & 65535) as usize]
}

/// The body's shape, which sets its box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pose {
    Standing,
    Sneaking,
    /// Swimming, gliding and crawling: 0.6 tall.
    Low,
}

impl Pose {
    pub const fn height(self) -> f32 {
        match self {
            Pose::Standing => 1.8,
            Pose::Sneaking => 1.49,
            Pose::Low => 0.6,
        }
    }

    /// The wire position is the feet plus this, in every pose.
    pub fn network_offset(self) -> f32 {
        NETWORK_OFFSET
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

/// What a block is to a moving player.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Block {
    /// Native block friction (0.6 for ordinary blocks).
    pub friction: f32,
    pub kind: BlockKind,
    /// Water in the block: still, flowing or the water of a waterlogged
    /// block.
    pub water: bool,
}

impl Block {
    /// Air, and what an unloaded block reads as.
    pub const AIR: Block = Block {
        friction: 0.6,
        kind: BlockKind::Air,
        water: false,
    };
}

/// The movement families of blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    Plain,
    Slime,
    Bed,
    Air,
    /// A cobweb: scales the move while the body is in it.
    Cobweb,
    /// Powder snow: scales the move while the feet are in it.
    PowderSnow,
    /// Ladders and vines: climbed at 0.2 a tick.
    Climbable,
    /// Soul sand: accelerates as if its friction were 1.225 times higher.
    SoulSand,
    /// Honey: slows like slime underfoot, and jumps lower.
    Honey,
    /// Soul soil: moved as a plain block, but soul speed acts on it (as on
    /// soul sand); soul speed is not simulated.
    SoulSoil,
}

impl BlockKind {
    /// The family of the block named `identifier` (any of its states).
    pub fn of(identifier: &str) -> Self {
        match identifier {
            "minecraft:slime" => Self::Slime,
            "minecraft:bed" => Self::Bed,
            "minecraft:air" => Self::Air,
            "minecraft:web" => Self::Cobweb,
            "minecraft:powder_snow" => Self::PowderSnow,
            "minecraft:ladder" | "minecraft:vine" => Self::Climbable,
            "minecraft:soul_sand" => Self::SoulSand,
            "minecraft:honey_block" => Self::Honey,
            "minecraft:soul_soil" => Self::SoulSoil,
            _ => Self::Plain,
        }
    }

    /// Whether BDS runs confirmed how this family moves a player, bit for
    /// bit (the movement differential's scenarios, `tools/movement-diff`):
    /// beds and powder snow are simulated, but not yet confirmed.
    pub fn confirmed(self) -> bool {
        !matches!(self, Self::Bed | Self::PowderSnow)
    }
}

/// What movement needs from the world.
pub trait Terrain {
    /// Append the collision boxes (world coordinates) of the blocks that can
    /// touch `area` to `out`. Blocks that are not loaded add nothing;
    /// callers check [`Terrain::loaded`] first.
    fn collisions(&self, area: &Aabb, out: &mut Vec<Aabb>);
    /// Every block of `area` is loaded.
    fn loaded(&self, area: &Aabb) -> bool;
    /// What the block at `pos` is to movement ([`Block::AIR`] when not
    /// loaded).
    fn block(&self, pos: [i32; 3]) -> Block;
}

/// The collision boxes that can touch `area`.
fn collisions(terrain: &impl Terrain, area: &Aabb) -> Vec<Aabb> {
    let mut out = Vec::new();
    terrain.collisions(area, &mut out);
    out
}

/// Water in any block `area` touches.
fn water(terrain: &impl Terrain, area: &Aabb) -> bool {
    let (min, max) = area.block_range();
    (min[0]..=max[0]).any(|x| {
        (min[1]..=max[1]).any(|y| (min[2]..=max[2]).any(|z| terrain.block([x, y, z]).water))
    })
}

/// How a block the body is in scales its move, if it does: powder snow at
/// the feet, a cobweb anywhere in the box.
fn stuck(terrain: &impl Terrain, body: &Aabb) -> Option<[f32; 3]> {
    let feet = [
        ((body.min[0] + body.max[0]) * 0.5).floor() as i32,
        body.min[1].floor() as i32,
        ((body.min[2] + body.max[2]) * 0.5).floor() as i32,
    ];
    if terrain.block(feet).kind == BlockKind::PowderSnow {
        return Some([0.9, 1.5, 0.9]);
    }
    let (min, max) = body.block_range();
    let web = (min[0]..=max[0]).any(|x| {
        (min[1]..=max[1])
            .any(|y| (min[2]..=max[2]).any(|z| terrain.block([x, y, z]).kind == BlockKind::Cobweb))
    });
    web.then_some([0.25, 0.05, 0.25])
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
    /// `minecraft:movement` without the sprint: 0.1 with the speed and
    /// slowness effects ([`walk_speed`]); a sprint multiplies it by 1.3.
    pub walk_speed: f32,
    /// The last tick moved as a sprinter.
    pub moved_sprinting: bool,
    pub jump_boost: u8,
    pub slow_falling: bool,
    /// The levitation effect's level (1-based, 0 for none).
    pub levitation: u8,
    /// The last tick's move collided horizontally / vertically.
    pub horizontal_collision: bool,
    pub vertical_collision: bool,
    /// The box the last move left, with the feet and pose it was left at.
    /// Moves translate the box and the feet are its centre, so a box rebuilt
    /// from the rounded feet differs by an ULP; BDS keeps the box.
    swept: Option<(Aabb, [f32; 3], Pose)>,
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
            walk_speed: WALK_SPEED,
            moved_sprinting: false,
            jump_boost: 0,
            slow_falling: false,
            levitation: 0,
            horizontal_collision: false,
            vertical_collision: false,
            swept: None,
        }
    }

    /// `minecraft:movement` while sprinting.
    pub fn sprint_speed(&self) -> f32 {
        self.walk_speed * SPRINT_MULTIPLIER
    }

    /// Set the speed without the sprint (an effect started or ended); the
    /// current speed follows, sprinting or not.
    pub fn set_walk_speed(&mut self, speed: f32) {
        self.walk_speed = speed;
        self.movement_speed = if self.sprinting {
            self.sprint_speed()
        } else {
            speed
        };
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

    /// The collision box: the one the last move left while the feet and
    /// pose are unchanged since, else rebuilt at the feet.
    pub fn aabb(&self) -> Aabb {
        match self.swept {
            Some((aabb, feet, pose)) if feet == self.feet && pose == self.pose() => aabb,
            _ => Aabb::from_feet(self.feet, WIDTH, self.pose().height()),
        }
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

/// `minecraft:movement` without the sprint under speed level `speed` and
/// slowness level `slowness` (1-based, 0 for none): BDS 1.26.51 sends 0.14
/// for speed II and 0.07 for slowness II, and 1.3 times that sprinting.
pub fn walk_speed(speed: u8, slowness: u8) -> f32 {
    WALK_SPEED * (1.0 + 0.2 * f32::from(speed)) * (1.0 - 0.15 * f32::from(slowness))
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
        body.movement_speed = body.sprint_speed();
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
pub fn step(body: &mut Body, controls: &Controls, terrain: &impl Terrain) -> Changes {
    let mut changes = Changes::default();
    // The sprint the last tick moved with: water drag keeps it a tick on.
    let was_sprinting = body.moved_sprinting;
    let stop_sprint = update_state(body, controls, &mut changes);
    if !terrain.loaded(&body.aabb().inflate([1.0, 1.0, 1.0])) {
        body.velocity = [0.0; 3];
        changes.frozen = true;
        finish_sprint(body, stop_sprint, &changes);
        return changes;
    }
    if body.velocity.iter().map(|v| v * v).sum::<f32>() < 1.0e-12 {
        body.velocity = [0.0; 3];
    }
    body.in_water = water(terrain, &body.water_box());
    if body.gliding && (body.on_ground || body.in_water) {
        body.gliding = false;
        changes.stopped_gliding = true;
    }
    if body.in_water {
        water_tick(body, controls, terrain, was_sprinting);
    } else if body.gliding {
        glide_tick(body, controls, terrain);
    } else {
        land_tick(body, controls, terrain, &mut changes);
    }
    if !controls.jump {
        body.jump_delay = 0;
    } else if body.jump_delay > 0 {
        body.jump_delay -= 1;
    }
    finish_sprint(body, stop_sprint, &changes);
    changes
}

/// After the move: remember the sprint it used, then end a stopping sprint
/// (and swim).
fn finish_sprint(body: &mut Body, stop: bool, changes: &Changes) {
    body.moved_sprinting = body.sprinting;
    if stop {
        body.sprinting = false;
        body.movement_speed = body.walk_speed;
    }
    if changes.stopped_swimming {
        body.swimming = false;
    }
}

/// The move vector's acceleration at `speed`, turned by the yaw.
fn move_relative(body: &mut Body, controls: &Controls, speed: f32) {
    let [strafe, forward] = impulse(body, controls);
    let force = forward * forward + strafe * strafe;
    if force < 1.0e-4 {
        return;
    }
    let scale = speed / force.sqrt().max(1.0);
    // The input turns by the exact sine and cosine (the sine table is only
    // the sprint jump's): yaw 37.5 matches BDS only this way.
    let (sin, cos) = controls.yaw.to_radians().sin_cos();
    let (strafe, forward) = (strafe * scale, forward * scale);
    body.velocity[0] += strafe * cos - forward * sin;
    body.velocity[2] += strafe * sin + forward * cos;
}

/// The move vector as the move uses it, `[strafe, forward]`: the keys as
/// sent, brought onto the unit circle when longer, slowed by the sneak
/// (a factor, so a diagonal stays diagonal), then scaled by 0.98.
fn impulse(body: &Body, controls: &Controls) -> [f32; 2] {
    let (mut strafe, mut forward) = (controls.strafe, controls.forward);
    let length_sq = forward * forward + strafe * strafe;
    if length_sq > 1.0 {
        let length = length_sq.sqrt();
        strafe /= length;
        forward /= length;
    }
    if body.sneaking {
        strafe *= SNEAK_IMPULSE;
        forward *= SNEAK_IMPULSE;
    }
    [strafe * IMPULSE_SCALE, forward * IMPULSE_SCALE]
}

/// The input acceleration on ground of friction `friction` (with the air's
/// 0.91): the speed times the cube of `0.546 / friction`, the cube rounded
/// before the speed multiplies it (BDS 1.26.51 accelerates exactly `speed`
/// on stone, where a precomputed 0.16277136 / f³ is an ULP off, and a sprint
/// on ice matches only this way).
fn ground_acceleration(speed: f32, friction: f32) -> f32 {
    let ratio = BASE_GROUND_FRICTION / friction;
    speed * (ratio * ratio * ratio)
}

/// A horizontal velocity after friction: one no larger than `f32::EPSILON`
/// stops, keeping its sign (a slide ends at 1.06e-7, not 5.8e-8, on BDS,
/// and a backward one at -0).
fn damp(velocity: f32, friction: f32) -> f32 {
    if velocity.abs() <= f32::EPSILON {
        0.0f32.copysign(velocity)
    } else {
        velocity * friction
    }
}

/// Whether the jump starts from honey: the block the feet are in, or the one
/// below when the feet rest on its top.
fn on_honey(body: &Body, terrain: &impl Terrain) -> bool {
    let [x, y, z] = body.feet.map(|v| v.floor() as i32);
    let honey = |y| terrain.block([x, y, z]).kind == BlockKind::Honey;
    honey(y) || (body.feet[1] == body.feet[1].floor() && honey(y - 1))
}

/// Whether the block the feet are in is climbable (a ladder or vines).
fn climbable(body: &Body, terrain: &impl Terrain) -> bool {
    terrain.block(body.feet.map(|v| v.floor() as i32)).kind == BlockKind::Climbable
}

/// The block under the feet that sets friction (half a block down).
/// Air there (the feet over an edge) counts as ordinary ground, not air's own
/// friction.
fn friction_block(body: &Body, terrain: &impl Terrain) -> Block {
    let block = terrain.block([
        body.feet[0].floor() as i32,
        (body.feet[1] - 0.5).floor() as i32,
        body.feet[2].floor() as i32,
    ]);
    if block.kind == BlockKind::Air {
        Block::AIR
    } else {
        block
    }
}

/// The block the feet rest on after a move.
fn landing_block(body: &Body, terrain: &impl Terrain) -> Block {
    let x = body.feet[0].floor() as i32;
    let z = body.feet[2].floor() as i32;
    let ground = terrain.block([x, (body.feet[1] - 0.2).floor() as i32, z]);
    if ground.kind == BlockKind::Air {
        terrain.block([x, body.feet[1].floor() as i32 - 1, z])
    } else {
        ground
    }
}

fn land_tick(body: &mut Body, controls: &Controls, terrain: &impl Terrain, changes: &mut Changes) {
    let mut friction = AIR_FRICTION;
    let mut speed = if body.sprinting {
        AIR_SPEED_SPRINTING
    } else {
        AIR_SPEED
    };
    if body.on_ground {
        let ground = friction_block(body, terrain);
        friction *= ground.friction;
        let pull = if ground.kind == BlockKind::SoulSand {
            ground.friction * SOUL_SAND_ACCELERATION * AIR_FRICTION
        } else {
            friction
        };
        speed = ground_acceleration(body.movement_speed, pull);
    }
    let climbing = climbable(body, terrain);
    // The jump comes before the input's acceleration: a sprint jump's push
    // is added to the velocity first (a sprint jump on ice matches BDS only
    // this way).
    if controls.jump && !climbing && body.on_ground && body.jump_delay == 0 {
        let mut jump = JUMP_VELOCITY + JUMP_BOOST_STEP * f32::from(body.jump_boost);
        if on_honey(body, terrain) {
            jump *= HONEY_JUMP;
        }
        body.velocity[1] = body.velocity[1].max(jump);
        body.jump_delay = JUMP_DELAY;
        if body.sprinting {
            let yaw = controls.yaw * 0.017_453_292;
            body.velocity[0] -= table_sin(yaw) * SPRINT_JUMP;
            body.velocity[2] += table_cos(yaw) * SPRINT_JUMP;
        }
        changes.jumped = true;
    }
    move_relative(body, controls, speed);
    if controls.jump && climbing {
        // The jump key climbs instead of jumping, also on the ground.
        body.velocity[1] = CLIMB_SPEED;
    }
    if climbing {
        // A climbable block holds the fall to 0.2 a tick, and a sneaking
        // climber in place.
        body.velocity[1] = body.velocity[1].max(-CLIMB_SPEED);
        if body.sneaking && body.velocity[1] < 0.0 {
            body.velocity[1] = 0.0;
        }
    }
    if body.sneaking && body.on_ground && body.velocity[1] <= 0.0 {
        avoid_edges(body, terrain);
    }
    let before = body.velocity;
    let was_on_ground = body.on_ground;
    let stuck = stuck(terrain, &body.aabb());
    if let Some(scale) = stuck {
        // A web or powder snow scales this move and stops the body after it.
        for (velocity, scale) in body.velocity.iter_mut().zip(scale) {
            *velocity *= scale;
        }
    }
    move_and_collide(body, terrain);
    if stuck.is_some() {
        body.velocity = [0.0; 3];
    }
    let ground = landing_block(body, terrain);
    if body.vertical_collision {
        body.velocity[1] = if !was_on_ground && before[1] < 0.0 && !controls.sneak {
            match ground.kind {
                // Slime returns the speed at impact: the speed the tick
                // started with, accelerated by gravity over the distance
                // fallen before the floor stopped the move (BDS bounces 0.8585
                // from 0.8439 after falling 0.1555); slower falls do not
                // bounce.
                BlockKind::Slime if -before[1] < SLIME_MIN_BOUNCE => 0.0,
                BlockKind::Slime => {
                    let fallen = -body.velocity[1];
                    (before[1] * before[1] + 2.0 * GRAVITY * fallen).sqrt()
                }
                BlockKind::Bed => (-0.66 * before[1]).min(1.0),
                _ => 0.0,
            }
        } else {
            0.0
        };
    }
    if body.horizontal_collision && climbable(body, terrain) {
        // Pushing into a wall from a climbable block climbs, with no gravity
        // or drag this tick.
        body.velocity[1] = CLIMB_SPEED;
    } else if body.levitation > 0 {
        // Levitation replaces gravity: the vertical velocity moves a fifth of
        // the way to 0.05 a level each tick, rounded as 0.8 v + 0.01 a level
        // (the only form that gives BDS's bits on all eleven measured
        // ticks), then drags.
        let lift = LEVITATION_LIFT * f32::from(body.levitation);
        body.velocity[1] = (body.velocity[1] * 0.8 + lift) * VERTICAL_DRAG;
    } else {
        // Slow falling lightens gravity only while falling: a body at rest
        // in the air takes a full 0.08 first.
        let gravity = if body.slow_falling && body.velocity[1] < 0.0 {
            SLOW_FALLING_GRAVITY
        } else {
            GRAVITY
        };
        body.velocity[1] = (body.velocity[1] - gravity) * VERTICAL_DRAG;
    }
    body.velocity[0] = damp(body.velocity[0], friction);
    body.velocity[2] = damp(body.velocity[2], friction);
    // Standing on slime or honey slows the horizontal velocity by the
    // vertical one after gravity (so 0.4 + 0.0784 x 0.2 on the ground).
    if body.on_ground
        && !body.sneaking
        && matches!(ground.kind, BlockKind::Slime | BlockKind::Honey)
    {
        let vertical = body.velocity[1].abs();
        if vertical < 0.1 {
            let slow = 0.4 + vertical * 0.2;
            body.velocity[0] *= slow;
            body.velocity[2] *= slow;
        }
    }
}

/// One tick in water: the swimmer's pitch, the jump's lift, 0.02
/// acceleration, drag 0.8 (0.9 for a sprinter, still in the tick its sprint
/// stops and the next) and a 0.005 sink unless sprinting.
fn water_tick(body: &mut Body, controls: &Controls, terrain: &impl Terrain, was_sprinting: bool) {
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
        if look_y <= 0.0 || controls.jump || water(terrain, &under.inflate([-0.01; 3])) {
            body.velocity[1] += (look_y - body.velocity[1]) * rate;
        }
    }
    if controls.jump {
        body.velocity[1] += WATER_JUMP;
    }
    move_relative(body, controls, WATER_SPEED);
    let y = body.feet[1];
    move_and_collide(body, terrain);
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
        if collisions(terrain, &probe).is_empty() && !water(terrain, &probe) {
            body.velocity[1] = 0.3;
        }
    }
}

fn glide_tick(body: &mut Body, controls: &Controls, terrain: &impl Terrain) {
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
    move_and_collide(body, terrain);
}

/// Sneaking on the ground: shorten the move until the body would still
/// stand on something (0.025 in from its sides, 0.05 at a time).
fn avoid_edges(body: &mut Body, terrain: &impl Terrain) {
    const INSET: f32 = 0.025;
    const STEP: f32 = 0.05;
    let bb = body.aabb().inflate([-INSET, 0.0, -INSET]);
    let drop = -STEP_HEIGHT * 1.01;
    let unsupported = |dx: f32, dz: f32| -> bool {
        let probe = bb.offset([dx, drop, dz]);
        collisions(terrain, &probe).is_empty()
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
    while dx != 0.0 && unsupported(dx, 0.0) {
        dx = shorten(dx);
    }
    while dz != 0.0 && unsupported(0.0, dz) {
        dz = shorten(dz);
    }
    while dx != 0.0 && dz != 0.0 && unsupported(dx, dz) {
        dx = shorten(dx);
        dz = shorten(dz);
    }
    body.velocity[0] = dx;
    body.velocity[2] = dz;
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
fn move_and_collide(body: &mut Body, terrain: &impl Terrain) {
    let velocity = body.velocity;
    let start = body.aabb();
    let boxes = collisions(terrain, &start.expand(velocity).inflate([1.0e-3; 3]));
    let (mut bb, mut moved) = clip_all(start, velocity, &boxes);
    let hit = |axis: usize, moved: &[f32; 3]| (velocity[axis] - moved[axis]).abs() >= 1.0e-5;
    let grounded = body.on_ground || (hit(1, &moved) && velocity[1] < 0.0);
    if grounded && (hit(0, &moved) || hit(2, &moved)) {
        let reach = start.expand([velocity[0], STEP_HEIGHT, velocity[2]]);
        let step_boxes: Vec<Aabb> = collisions(terrain, &reach.inflate([1.0e-3; 3]))
            .into_iter()
            .filter(|b| b.min[1] < start.max[1])
            .collect();
        let (up_bb, up) = clip_all(start, [0.0, STEP_HEIGHT, 0.0], &step_boxes);
        let (across_bb, across) = clip_all(up_bb, [velocity[0], 0.0, velocity[2]], &step_boxes);
        let (down_bb, down) = clip_all(across_bb, [0.0, -up[1], 0.0], &step_boxes);
        let stepped = [across[0], up[1] + down[1], across[2]];
        let blocked = collisions(terrain, &down_bb)
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
    body.swept = Some((bb, body.feet, body.pose()));
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
}

/// Whether a body box at `bb` overlaps any block's collision box.
pub fn collides(terrain: &impl Terrain, bb: &Aabb) -> bool {
    collisions(terrain, bb).iter().any(|b| b.intersects(bb))
}
