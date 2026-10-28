//! Per-entity-type physics parameters.

use bevy_ecs::prelude::*;

use super::{Age, ProjectileData};

/// When gravity is applied relative to the collision move and drag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GravityStage {
    /// `v.y -= g; move(v); v *= drag` (items, falling blocks).
    BeforeMove,
    /// `move(v); v.y = (v.y - g) * drag` (living entities).
    AfterMove,
    /// `move(v); v *= drag; v.y -= g` (projectiles).
    AfterDrag,
}

/// Built-in motion rules that are not interchangeable box/gravity parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhysicsModel {
    Generic,
    /// Liquid motion and the measured low-speed four-tick cadence.
    Item,
    /// Falling blocks stop all motion on the landing tick.
    FallingBlock,
}

/// Extra horizontal friction while standing on the ground.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GroundFriction {
    /// No extra friction on the ground.
    None,
    /// Multiply by the native `friction` of the supporting block
    /// (0.6 for most blocks, 0.98 for ice, 0.8 for slime).
    Block,
}

/// Physics body: collision box, gravity, drag and step-up for an entity.
///
/// Entities with `Position`, `Velocity`, `OnGround` and a `PhysicsBody` are
/// moved by [`crate::entity::systems::tick_physics`] against block collision
/// shapes. A body requires `Age` for per-actor item tick cadence. Players are
/// client-authoritative and do not carry a body.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
#[require(Age)]
pub struct PhysicsBody {
    pub model: PhysicsModel,
    /// Box width (x and z) in blocks.
    pub width: f64,
    /// Box height in blocks.
    pub height: f64,
    /// Vertical offset between the feet position and the position the
    /// Bedrock client expects on the wire for this entity type.
    pub network_y_offset: f64,
    /// Gravity subtracted from `v.y` every tick.
    pub gravity: f64,
    /// Ordering of gravity, move and drag.
    pub gravity_stage: GravityStage,
    /// Per-tick multiplier applied to `v.y`.
    pub vertical_drag: f64,
    /// Per-tick multiplier applied to `v.x`/`v.z`.
    pub horizontal_drag: f64,
    /// Extra horizontal friction on the ground.
    pub ground_friction: GroundFriction,
    /// Maximum height the entity steps up when walking into a block.
    pub step_height: f64,
}

impl PhysicsBody {
    /// Dropped item: measured 0.25 box and 0.125 wire offset, 0.04 pre-move
    /// gravity and 0.98 post-move drag. Native block friction also applies
    /// on the ground. Liquid response and low-speed cadence are recorded in
    /// `systems/testdata/bds-1.26.51-contact.json`.
    pub const ITEM: Self = Self {
        model: PhysicsModel::Item,
        width: 0.25,
        height: 0.25,
        network_y_offset: 0.125,
        gravity: 0.04,
        gravity_stage: GravityStage::BeforeMove,
        vertical_drag: 0.98,
        horizontal_drag: 0.98,
        ground_friction: GroundFriction::Block,
        step_height: 0.0,
    };

    /// Falling block: measured 0.98 box, 0.49 wire offset, 0.04 pre-move
    /// gravity and 0.98 drag. A native post-impulse trace confirms that all
    /// velocity is zeroed on landing, not multiplied by an assumed friction.
    pub const FALLING_BLOCK: Self = Self {
        model: PhysicsModel::FallingBlock,
        width: 0.98,
        height: 0.98,
        network_y_offset: 0.49,
        gravity: 0.04,
        gravity_stage: GravityStage::BeforeMove,
        vertical_drag: 0.98,
        horizontal_drag: 0.98,
        ground_friction: GroundFriction::None,
        step_height: 0.0,
    };

    /// Generic living entity: gravity 0.08 after the move, 0.98 vertical drag,
    /// 0.91 horizontal air friction, block friction on the ground and a 0.6
    /// block step-up.
    pub fn living(width: f64, height: f64) -> Self {
        Self {
            model: PhysicsModel::Generic,
            width,
            height,
            network_y_offset: 0.0,
            gravity: 0.08,
            gravity_stage: GravityStage::AfterMove,
            vertical_drag: 0.98,
            horizontal_drag: 0.91,
            ground_friction: GroundFriction::Block,
            step_height: 0.6,
        }
    }

    /// Projectile using its configured gravity and drag.
    pub fn projectile(width: f64, height: f64, data: &ProjectileData) -> Self {
        Self {
            model: PhysicsModel::Generic,
            width,
            height,
            network_y_offset: 0.0,
            gravity: data.gravity,
            gravity_stage: GravityStage::AfterDrag,
            vertical_drag: 1.0 - data.drag,
            horizontal_drag: 1.0 - data.drag,
            ground_friction: GroundFriction::None,
            step_height: 0.0,
        }
    }
}

/// A block in free fall (`minecraft:falling_block`).
///
/// Lands as `state` where it stops, or drops as an item when it cannot.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct FallingBlock {
    /// Runtime ID of the falling block state.
    pub state: u32,
}
