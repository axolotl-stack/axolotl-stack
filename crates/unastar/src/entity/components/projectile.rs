//! Projectile components.

use bevy_ecs::prelude::*;

/// Marker for projectile entities.
#[derive(Component, Debug)]
pub struct Projectile;

/// Projectile type and configuration.
#[derive(Component, Debug, Clone)]
pub struct ProjectileData {
    pub owner: Option<Entity>,
    pub damage: f32,
    pub gravity: f64,
    pub drag: f64,
    pub critical: bool,
    pub pierce_level: u8,
}

impl Default for ProjectileData {
    fn default() -> Self {
        Self {
            owner: None,
            damage: 2.0,
            gravity: 0.05,
            drag: 0.01,
            critical: false,
            pierce_level: 0,
        }
    }
}

impl ProjectileData {
    pub fn arrow() -> Self {
        Self {
            damage: 2.0,
            gravity: 0.05,
            drag: 0.01,
            ..Default::default()
        }
    }

    pub fn snowball() -> Self {
        Self {
            damage: 0.0,
            gravity: 0.03,
            drag: 0.01,
            ..Default::default()
        }
    }

    pub fn fireball() -> Self {
        Self {
            damage: 6.0,
            gravity: 0.0, // Fireballs don't have gravity
            drag: 0.0,
            ..Default::default()
        }
    }

    pub fn with_owner(mut self, owner: Entity) -> Self {
        self.owner = Some(owner);
        self
    }

    pub fn with_damage(mut self, damage: f32) -> Self {
        self.damage = damage;
        self
    }

    pub fn with_critical(mut self, critical: bool) -> Self {
        self.critical = critical;
        self
    }
}

/// Whether the projectile has hit something.
#[derive(Component, Debug, Default)]
pub struct ProjectileHit {
    pub hit_entity: Option<Entity>,
    pub hit_block: Option<glam::IVec3>,
    pub stuck_ticks: u32,
}

impl ProjectileHit {
    pub fn is_stuck(&self) -> bool {
        self.hit_block.is_some()
    }
}

/// Pickup state for arrows.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickupMode {
    /// Cannot be picked up.
    None,
    /// Only the owner can pick up.
    Owner,
    /// Anyone can pick up.
    Anyone,
    /// Creative only.
    Creative,
}

impl Default for PickupMode {
    fn default() -> Self {
        Self::Anyone
    }
}
