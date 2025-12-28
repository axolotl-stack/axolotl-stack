//! Spatial transform components.

use bevy_ecs::prelude::*;
use glam::DVec3;

/// World position in double precision (blocks).
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct Position(pub DVec3);

impl Position {
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Self(DVec3::new(x, y, z))
    }

    /// Convert to protocol Vec3F.
    pub fn to_protocol(&self) -> jolyne::valentine::types::Vec3F {
        jolyne::valentine::types::Vec3F {
            x: self.0.x as f32,
            y: self.0.y as f32,
            z: self.0.z as f32,
        }
    }

    /// Create from protocol Vec3F.
    pub fn from_protocol(v: &jolyne::valentine::types::Vec3F) -> Self {
        Self(DVec3::new(v.x as f64, v.y as f64, v.z as f64))
    }
}

/// Velocity in blocks per tick.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct Velocity(pub DVec3);

impl Velocity {
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Self(DVec3::new(x, y, z))
    }
}

/// Rotation in degrees (yaw, pitch, head_yaw).
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct Rotation {
    pub yaw: f32,
    pub pitch: f32,
    pub head_yaw: f32,
}

impl Rotation {
    pub fn new(yaw: f32, pitch: f32) -> Self {
        Self {
            yaw,
            pitch,
            head_yaw: yaw,
        }
    }

    pub fn with_head_yaw(yaw: f32, pitch: f32, head_yaw: f32) -> Self {
        Self {
            yaw,
            pitch,
            head_yaw,
        }
    }
}

/// Head yaw for players/mobs (separate from body rotation).
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct HeadYaw(pub f32);

/// Whether the entity is on the ground.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct OnGround(pub bool);

/// Unique runtime ID for network protocol.
#[derive(Component, Debug, Clone, Copy)]
pub struct RuntimeId(pub i64);

impl RuntimeId {
    pub fn new(id: i64) -> Self {
        Self(id)
    }
}

/// Entity age in ticks.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct Age(pub u64);

impl Age {
    pub fn tick(&mut self) {
        self.0 = self.0.wrapping_add(1);
    }
}
